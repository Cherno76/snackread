#import <Foundation/Foundation.h>
#import <UIKit/UIKit.h>
#import <UniformTypeIdentifiers/UniformTypeIdentifiers.h>
#import <objc/runtime.h>

#include "bindings/bindings.h"

// ---------------------------------------------------------------------------
// iOS 外部文件夹选择
//
// iOS 应用是沙盒：容器外的目录（iCloud Drive、其他 App 的「我的 iPhone」文件夹、
// 外接存储等）不能直接 fs::read_dir。唯一正路是让用户通过系统选择器授权，拿到
// security-scoped URL 后 startAccessingSecurityScopedResource，并保存
// security-scoped bookmark 供下次启动恢复权限。
// ---------------------------------------------------------------------------

// 回调约定：path 为 NULL 表示用户取消；否则 path 是文件夹绝对路径，
// bookmark/len 是该目录的 security-scoped bookmark（可能为空）。
typedef void (*snackread_dir_cb)(const char *path, const unsigned char *bookmark, unsigned long len, void *ctx);
typedef void (*snackread_pick_fn)(snackread_dir_cb cb, void *ctx);
// 解析 bookmark 并开启安全作用域访问，返回它**当前**指向的路径（调用方负责释放）
typedef char *(*snackread_restore_fn)(const unsigned char *bookmark, unsigned long len);
typedef void (*snackread_free_fn)(char *ptr);
typedef void (*snackread_statusbar_fn)(int hidden);

// Rust 侧（src/lib.rs 的 snackread_register_bridge）在启动时接收这两个实现。
// 刻意做成 ObjC → Rust 的方向：Rust 只保存函数指针，不引用 main.o 里的符号，
// 否则 iOS 上 cdylib 的链接会因为没有定义 _snackread_pick_folder 而失败。
extern "C" void snackread_register_bridge(snackread_pick_fn pick,
                                          snackread_restore_fn restore,
                                          snackread_free_fn free_str,
                                          snackread_statusbar_fn statusbar);

// 已授权的 URL 必须一直持有，security scope 随 URL 的释放而失效。
static NSMutableArray<NSURL *> *gScopedURLs = nil;
// 选择器是异步的，delegate 要活到回调结束（否则会被提前释放）。
static id gPickerDelegate = nil;

static void snackread_keep_scoped(NSURL *url) {
  if (url == nil) return;
  if (gScopedURLs == nil) gScopedURLs = [NSMutableArray array];
  [gScopedURLs addObject:url];
}

static UIViewController *snackread_top_view_controller(void) {
  UIWindow *window = nil;
  for (UIScene *scene in UIApplication.sharedApplication.connectedScenes) {
    if (![scene isKindOfClass:[UIWindowScene class]]) continue;
    UIWindowScene *windowScene = (UIWindowScene *)scene;
    for (UIWindow *w in windowScene.windows) {
      if (w.isKeyWindow) {
        window = w;
        break;
      }
    }
    if (window == nil && windowScene.windows.count > 0) window = windowScene.windows.firstObject;
    if (window != nil) break;
  }
  UIViewController *vc = window.rootViewController;
  while (vc.presentedViewController != nil) vc = vc.presentedViewController;
  return vc;
}

@interface SnackReadFolderPicker : NSObject <UIDocumentPickerDelegate>
@property(nonatomic) snackread_dir_cb cb;
@property(nonatomic) void *ctx;
@end

@implementation SnackReadFolderPicker

- (void)finishWithURL:(NSURL *)url {
  snackread_dir_cb cb = self.cb;
  void *ctx = self.ctx;
  self.cb = NULL;
  self.ctx = NULL;
  if (cb == NULL) return;
  if (url == nil) {
    cb(NULL, NULL, 0, ctx);
    return;
  }
  [url startAccessingSecurityScopedResource];
  snackread_keep_scoped(url);
  NSData *bookmark = [url bookmarkDataWithOptions:0
                    includingResourceValuesForKeys:nil
                                     relativeToURL:nil
                                             error:NULL];
  const char *path = url.path.UTF8String;
  cb(path, (const unsigned char *)bookmark.bytes, (unsigned long)bookmark.length, ctx);
}

- (void)documentPicker:(UIDocumentPickerViewController *)controller
    didPickDocumentsAtURLs:(NSArray<NSURL *> *)urls {
  [self finishWithURL:urls.firstObject];
  gPickerDelegate = nil;
}

- (void)documentPickerWasCancelled:(UIDocumentPickerViewController *)controller {
  [self finishWithURL:nil];
  gPickerDelegate = nil;
}

@end

/// 弹出系统「选择文件夹」选择器，选中后把路径与 bookmark 交给 cb。
/// 回调在主线程；取消时 path 为 NULL。
static void snackread_pick_folder(snackread_dir_cb cb, void *ctx) {
  dispatch_async(dispatch_get_main_queue(), ^{
    UIViewController *root = snackread_top_view_controller();
    if (root == nil) {
      cb(NULL, NULL, 0, ctx);
      return;
    }

    SnackReadFolderPicker *delegate = [SnackReadFolderPicker new];
    delegate.cb = cb;
    delegate.ctx = ctx;
    gPickerDelegate = delegate;

    UIDocumentPickerViewController *picker =
        [[UIDocumentPickerViewController alloc] initForOpeningContentTypes:@[ UTTypeFolder ]];
    picker.delegate = delegate;
    picker.allowsMultipleSelection = NO;
    [root presentViewController:picker animated:YES completion:nil];
  });
}

/// 用上次保存的 bookmark 重新打开授权（失败返回 NULL）。
/// 返回解析出的路径：bookmark 比路径稳，目录位置变了它也能解析回同一个位置，
/// 所以工作目录这类要跨启动使用的位置必须以这个返回值为准。
static char *snackread_restore_bookmark(const unsigned char *bytes, unsigned long len) {
  if (bytes == NULL || len == 0) return NULL;
  NSData *bookmark = [NSData dataWithBytes:bytes length:len];
  BOOL stale = NO;
  NSURL *url = [NSURL URLByResolvingBookmarkData:bookmark
                                        options:0
                                  relativeToURL:nil
                            bookmarkDataIsStale:&stale
                                          error:NULL];
  if (url == nil) return NULL;
  [url startAccessingSecurityScopedResource];
  snackread_keep_scoped(url);
  const char *path = url.path.UTF8String;
  return path == NULL ? NULL : strdup(path);
}

static void snackread_free_string(char *ptr) {
  if (ptr != NULL) free(ptr);
}

// ---------------------------------------------------------------------------
// 系统状态栏显隐（阅读模式沉浸式）
//
// Info.plist 没设 UIViewControllerBasedStatusBarAppearance，默认 YES，即系统状态栏由
// 「状态栏控制器」（窗口的根视图控制器）的 prefersStatusBarHidden 决定。
// 根控制器是 tao 在 Rust 里创建的，没法子类化，所以给这个实例换上一个带
// prefersStatusBarHidden 覆写的动态子类（isa-swizzling）：
//   - 比在 UIViewController 上加 category 影响面小，不会波及文件选择器等其它控制器；
//   - 不改变视图层级，也就不影响旋转方向等交给根控制器的决策。
// ---------------------------------------------------------------------------

static BOOL gStatusBarHidden = NO;
static Class gStatusBarSubclass = Nil;

static BOOL snackread_prefers_status_bar_hidden(id self, SEL _cmd) {
  return gStatusBarHidden;
}

static UIWindow *snackread_key_window(void) {
  for (UIScene *scene in UIApplication.sharedApplication.connectedScenes) {
    if (![scene isKindOfClass:[UIWindowScene class]]) continue;
    UIWindowScene *windowScene = (UIWindowScene *)scene;
    for (UIWindow *w in windowScene.windows) {
      if (w.isKeyWindow) return w;
    }
    if (windowScene.windows.count > 0) return windowScene.windows.firstObject;
  }
  return nil;
}

static void snackread_apply_status_bar(void) {
  UIViewController *root = snackread_key_window().rootViewController;
  if (root == nil) return;

  if (gStatusBarSubclass == Nil) {
    Class base = object_getClass(root);
    const char *baseName = class_getName(base);
    if (baseName == NULL) return;
    char name[256];
    snprintf(name, sizeof(name), "SnackReadStatusBar_%s", baseName);
    Class cls = objc_allocateClassPair(base, name, 0);
    if (cls == Nil) return; // 名字已存在（理论上不会）就放弃，保持原样
    Method method = class_getInstanceMethod(base, @selector(prefersStatusBarHidden));
    if (method == NULL) {
      objc_disposeClassPair(cls);
      return;
    }
    class_addMethod(cls, @selector(prefersStatusBarHidden),
                    (IMP)snackread_prefers_status_bar_hidden,
                    method_getTypeEncoding(method));
    objc_registerClassPair(cls);
    gStatusBarSubclass = cls;
  }

  if (object_getClass(root) != gStatusBarSubclass) {
    object_setClass(root, gStatusBarSubclass);
  }
  [root setNeedsStatusBarAppearanceUpdate];
}

static void snackread_set_status_bar_hidden(int hidden) {
  gStatusBarHidden = hidden != 0;
  dispatch_async(dispatch_get_main_queue(), ^{
    snackread_apply_status_bar();
  });
}

int main(int argc, char * argv[]) {
	snackread_register_bridge(snackread_pick_folder, snackread_restore_bookmark,
	                         snackread_free_string, snackread_set_status_bar_hidden);
	ffi::start_app();
	return 0;
}
