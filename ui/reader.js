/* cshow-gui 文字 EPUB 阅读器脚本（解包时注入各章节）
 * 与父窗口（app.js）通过 postMessage 通信：
 *   iframe -> parent: { cshowH } 滚动高度 / { cshowGeom:{chapter,pages,pageW,gap} } 分页几何 / { cshowReady:{chapter,mode} }
 *   parent -> iframe: { cshow:'reader', type:'set'|'goto'|'measure', cfg?, page? }
 */
(function () {
  'use strict';
  var doc = document;
  var root = doc.documentElement;
  var body = doc.body;

  var THEMES = {
    light: { bg: '#ffffff', fg: '#22252a', link: '#0b66c3' },
    sepia: { bg: '#f4ecd8', fg: '#4b3a26', link: '#8a5a1d' },
    dark:  { bg: '#14171c', fg: '#c9cdd3', link: '#6ba7ff' }
  };
  // 双页分割线颜色（跟随主题，半透明）
  var DIVIDERS = {
    light: 'rgba(34,37,42,0.18)',
    sepia: 'rgba(75,58,38,0.22)',
    dark:  'rgba(201,205,211,0.18)'
  };

  var SERIF = 'Georgia, "Songti SC", "Noto Serif CJK SC", "Source Han Serif SC", "Times New Roman", serif';
  var SANS = '-apple-system, BlinkMacSystemFont, "PingFang SC", "Noto Sans CJK SC", "Microsoft YaHei", sans-serif';

  function parseSearch(s) {
    var o = {};
    if (!s) return o;
    s = s.replace(/^\?/, '');
    var parts = s.split('&');
    for (var i = 0; i < parts.length; i++) {
      var eq = parts[i].indexOf('=');
      if (eq > 0) {
        try { o[decodeURIComponent(parts[i].slice(0, eq))] = decodeURIComponent(parts[i].slice(eq + 1)); } catch (e) {}
      }
    }
    return o;
  }

  var q = parseSearch(location.search);
  var chapter = parseInt(q.c || '0', 10);
  if (isNaN(chapter)) chapter = 0;

  var state = {
    mode: q.m === 'flip' ? 'flip' : 'scroll',
    theme: q.t || 'light',
    fs: parseInt(q.fs || '16', 10) || 16,
    ff: q.ff || 'system',
    lh: parseFloat(q.lh || '1.7') || 1.7,
    mg: parseInt(q.mg || '28', 10) || 28,
    mgB: parseInt(q.mgb || '28', 10) || 28,
    pageW: parseInt(q.pw || '0', 10) || 0,
    pageH: parseInt(q.ph || '0', 10) || 0,
    gap: parseInt(q.g || '24', 10) || 0
  };

  var wrap = null;
  var wrapBuilt = false;
  var measureTimer = null;

  function fontStack(ff) {
    if (ff === 'serif') return SERIF;
    if (ff === 'sans') return SANS;
    return ''; // system：不覆盖，交给 EPUB 自带字体
  }

  function applyStyle() {
    var t = THEMES[state.theme] || THEMES.light;
    var s = doc.getElementById('cshow-reader-style');
    if (!s) {
      s = doc.createElement('style');
      s.id = 'cshow-reader-style';
      (doc.head || root).appendChild(s);
    }
    // 视口宽（iframe 内部）：分页容器必须撑满，否则 EPUB 自带的 body max-width 会把列宽算错
    var vw = window.innerWidth || root.clientWidth || state.pageW || 800;
    var css = '';
    css += 'html,body{background:' + t.bg + ' !important;color:' + t.fg + ' !important;margin:0 !important;}';
    css += '[data-cshow-hdr]{display:none !important;}'; // 解包标记的固定页眉/页脚
    css += 'body a{color:' + t.link + ' !important;}';
    // 字号/行高/字体要直接应用到正文元素（而不只 body）：
    // 很多书的 CSS 给 p/li/blockquote 等设置了显式 font-size/font-family，
    // 元素自身的值会覆盖 body 的继承值，导致切换字号/字体对部分书无效。
    // 标题（h1-h6）、等宽代码（pre/code）、图注等特殊元素保留书的样式。
    css += 'body,p,li,blockquote,td,th{font-size:' + state.fs + 'px !important;line-height:' + state.lh + ' !important;}';
    var stack = fontStack(state.ff);
    if (stack) {
      css += 'body,p,li,blockquote,td,th{font-family:' + stack + ' !important;}';
      // 很多书的正文是 p > span 结构（如 <p><span class="x">文字</span></p>），
      // span 自己设了 font-family（如宋体）会压过 p 的继承，所以段落内 span 也要覆盖；
      // 标题/图注/pre 等特殊区域的 span 不动，保留书的排版。
      css += 'p span,li span,blockquote span,td span,th span{font-family:' + stack + ' !important;}';
    }
    // 图片宽度限视口；高度约束只在翻页模式加（见下方 flip 分支）
    css += 'img,svg,video{max-width:100%;}';
    // WebKit 多列用 -webkit-column-* 前缀实现，无前缀 break-inside 可能不生效
    css += '.cshow-pages figure,.cshow-pages table,.cshow-pages pre,.cshow-pages blockquote,.cshow-pages img,.cshow-pages svg{-webkit-column-break-inside:avoid;break-inside:avoid;}';
    if (state.mode === 'flip' && state.pageW > 0 && state.pageH > 0) {
      // 翻页模式强制全宽（覆盖 EPUB 的 body max-width），否则列宽按被压缩后的容器算，双页退化成一页
      css += 'html,body{width:100% !important;max-width:none !important;padding:0 !important;}';
      css += '.cshow-pages{';
      css += 'width:' + Math.max(200, vw - 2 * state.mg) + 'px;';
      css += 'margin:0 auto;';
      css += 'column-width:' + state.pageW + 'px;';
      css += 'column-gap:' + state.gap + 'px;';
      if (state.gap > 0) {
        // 双页：中间加大间距并画分割线
        css += '-webkit-column-rule:1px solid ' + (DIVIDERS[state.theme] || DIVIDERS.light) + ';';
        css += 'column-rule:1px solid ' + (DIVIDERS[state.theme] || DIVIDERS.light) + ';';
      }
      css += 'height:' + state.pageH + 'px;';
      css += 'column-fill:auto;-webkit-column-fill:auto;';
      // 横向滚动由父窗口统一翻页（gotoPage 用 scrollLeft 定位）；
      // 用 hidden 而非 auto，避免触控板滚轮原生滚动列容器导致落到半页位置
      css += 'overflow-x:hidden;overflow-y:hidden;';
      css += 'padding:' + state.mg + 'px 0 ' + state.mgB + 'px;';
      css += 'box-sizing:border-box;';
      css += '}';
      // WebKit 多列（已知 bug #25633）对替换元素（img）的尺寸计算不可靠：
      // 会把图片拉伸/错算宽度导致右侧被容器裁剪（看起来像“截断”），
      // 且单靠 img 上的 break-inside 无法解决。
      // 实测绕开方案：把图片放进 flex 容器，或强制撑满容器 + object-fit，
      // WebKit 在这些布局下按自然尺寸/等比完整渲染。
      var colH = Math.max(120, state.pageH - state.mg - state.mgB); // 列内容高（.cshow-pages 有上下 padding）
      css += '.cshow-imgguard,.cshow-imgpage{-webkit-column-break-inside:avoid;break-inside:avoid;}';
      // 大图（封面等）：容器撑满列内容高、图片 object-fit 等比完整显示、独占一列
      css += '.cshow-imgpage{height:' + colH + 'px;-webkit-column-break-before:always;-webkit-column-break-after:always;break-before:column;break-after:column;}';
      css += '.cshow-imgpage img,.cshow-imgpage svg,.cshow-imgpage video{width:100%;height:100%;object-fit:contain;display:block;margin:0 auto;}';
      // 小图：flex 容器绕开多列尺寸 bug，保持自然尺寸
      css += '.cshow-imgguard{display:flex;flex-direction:column;align-items:center;justify-content:center;}';
      css += '.cshow-imgguard img,.cshow-imgguard svg,.cshow-imgguard video{max-width:100%;max-height:' + Math.round(state.pageH * 0.55) + 'px;display:block;margin:0 auto;}';
    } else {
      // 滚动（条漫）模式：正文左右边距与分页一致
      css += 'html,body{padding:0 ' + state.mg + 'px !important;}';
    }
    s.textContent = css;
  }

  function buildWrap() {
    if (wrapBuilt) return;
    wrap = doc.createElement('div');
    wrap.className = 'cshow-pages';
    while (body.firstChild) wrap.appendChild(body.firstChild);
    body.appendChild(wrap);
    wrapBuilt = true;
    body.style.overflow = 'hidden';
    // 图片加载后才知真实高度：加载完重新标记（大图独占一列），并重新测量列数
    wrap.addEventListener('load', function (e) {
      var t = e.target;
      if (t && (t.tagName === 'IMG' || t.tagName === 'SVG')) {
        guardImages();
        scheduleMeasure();
      }
    }, true);
  }

  // 翻页模式：给含图容器标记防劈类；高度超过列高 60% 的图独占一列，
  // 保证图片完整显示（WebKit 多列对 img 的分割行为不可靠）
  function guardImages() {
    if (!wrap || state.mode !== 'flip') return;
    var list = wrap.querySelectorAll('img, svg, video');
    for (var i = 0; i < list.length; i++) {
      var el = list[i];
      var parent = el.parentElement || el;
      if (!parent || !parent.classList) continue;
      var h = 0;
      if (el.tagName === 'IMG') h = el.naturalHeight || 0;
      if (h <= 0 && el.getBoundingClientRect) {
        try { h = el.getBoundingClientRect().height; } catch (e) {}
      }
      var big = h > state.pageH * 0.6;
      parent.classList.remove('cshow-imgguard', 'cshow-imgpage');
      parent.classList.add(big ? 'cshow-imgpage' : 'cshow-imgguard');
    }
  }

  function teardownWrap() {
    if (!wrapBuilt) return;
    if (wrap) {
      while (wrap.firstChild) body.appendChild(wrap.firstChild);
      if (wrap.parentNode) wrap.parentNode.removeChild(wrap);
    }
    wrap = null;
    wrapBuilt = false;
    body.style.overflow = '';
  }

  function reportHeight() {
    var h = Math.max(root.scrollHeight, body ? body.scrollHeight : 0, 1);
    parent.postMessage({ cshowH: h }, '*');
  }

  function reportGeom() {
    if (state.mode !== 'flip') return;
    if (!wrapBuilt) buildWrap();
    var pw = state.pageW || wrap.clientWidth || 1;
    var gap = state.gap || 0;
    var pages = Math.max(1, Math.round((wrap.scrollWidth + gap) / (pw + gap)));
    parent.postMessage({ cshowGeom: { chapter: chapter, pages: pages, pageW: pw, gap: gap } }, '*');
  }

  function scheduleMeasure() {
    if (measureTimer) clearTimeout(measureTimer);
    measureTimer = setTimeout(function () {
      if (state.mode === 'flip') reportGeom();
      else reportHeight();
    }, 30);
  }

  function apply(cfg) {
    var wasFlip = state.mode === 'flip';
    if (cfg.mode === 'flip') state.mode = 'flip';
    else if (cfg.mode === 'scroll') state.mode = 'scroll';
    if (cfg.theme !== undefined) state.theme = cfg.theme;
    if (cfg.fs !== undefined) state.fs = cfg.fs;
    if (cfg.ff !== undefined) state.ff = cfg.ff;
    if (cfg.lh !== undefined) state.lh = cfg.lh;
    if (cfg.mg !== undefined) state.mg = cfg.mg;
    if (cfg.mgB !== undefined) state.mgB = cfg.mgB;
    if (cfg.pageW !== undefined) state.pageW = cfg.pageW;
    if (cfg.pageH !== undefined) state.pageH = cfg.pageH;
    if (cfg.gap !== undefined) state.gap = cfg.gap;
    if (state.mode === 'flip') {
      buildWrap();
      body.style.overflow = 'hidden';
    } else {
      teardownWrap();
    }
    applyStyle();
    guardImages(); // 图可能尚未加载，加载后由 wrap 的 load 监听再处理
    scheduleMeasure();
  }

  var scrollAnimToken = 0;
  // 缓动滚动：章节内翻页横向滑动（更慢更柔；新翻页会打断旧动画）
  function smoothScrollTo(el, x, duration) {
    var token = ++scrollAnimToken;
    var start = el.scrollLeft;
    var dist = x - start;
    if (Math.abs(dist) < 1) return;
    var t0 = (typeof performance !== 'undefined') ? performance.now() : Date.now();
    function ease(p) { return p < 0.5 ? 4 * p * p * p : 1 - Math.pow(-2 * p + 2, 3) / 2; }
    function step() {
      if (token !== scrollAnimToken) return; // 被更新的翻页打断
      var now = (typeof performance !== 'undefined') ? performance.now() : Date.now();
      var p = Math.min(1, (now - t0) / duration);
      el.scrollLeft = start + dist * ease(p);
      if (p < 1) requestAnimationFrame(step);
    }
    requestAnimationFrame(step);
  }

  function gotoPage(col) {
    if (state.mode !== 'flip') return;
    if (!wrapBuilt) buildWrap();
    var x = Math.max(0, Math.floor(col)) * (state.pageW + state.gap);
    if (wrap) smoothScrollTo(wrap, x, 300); // 横向滑动过渡
  }

  // 定位到文件内锚点（目录跳转）：滚动模式直接滚到元素；
  // 翻页模式把元素所在列号上报父窗口统一定位
  function gotoAnchor(anchor) {
    if (!anchor) return;
    var el = null;
    try { el = doc.getElementById(anchor); } catch (e) {}
    if (!el) return;
    if (state.mode === 'flip') {
      if (!wrapBuilt) buildWrap();
      var wr = wrap.getBoundingClientRect();
      var r = el.getBoundingClientRect();
      var x = r.left - wr.left;
      var col = Math.max(0, Math.floor(x / (state.pageW + state.gap)));
      parent.postMessage({ cshowAnchor: { chapter: chapter, col: col } }, '*');
    } else {
      el.scrollIntoView({ block: 'start' });
      parent.postMessage({ cshowAnchorDone: true }, '*');
    }
  }

  window.addEventListener('message', function (e) {
    var d = e.data;
    if (!d || typeof d !== 'object' || d.cshow !== 'reader') return;
    if (d.type === 'set') apply(d.cfg || {});
    else if (d.type === 'goto') gotoPage(d.page);
    else if (d.type === 'measure') scheduleMeasure();
    else if (d.type === 'anchor') gotoAnchor(d.anchor);
    else if (d.type === 'anchorcols') {
      // 上报目录锚点在当前章节内的列号（目录高亮用，一文件多章的书精确选中当前条目）
      if (state.mode !== 'flip') return;
      if (!wrapBuilt) buildWrap();
      var wr = wrap.getBoundingClientRect();
      var cols = {};
      var list = d.anchors || [];
      for (var i = 0; i < list.length; i++) {
        try {
          var el = doc.getElementById(list[i]);
          if (el) {
            var r = el.getBoundingClientRect();
            var x = r.left - wr.left;
            cols[list[i]] = Math.max(0, Math.floor(x / (state.pageW + state.gap)));
          }
        } catch (e) {}
      }
      parent.postMessage({ cshowAnchorCols: { chapter: chapter, cols: cols } }, '*');
    }
  });

  // 翻页模式：触控板滚轮不在这里原生滚动（避免落到半列），把 deltaX 交给父窗口统一按整页翻
  document.addEventListener('wheel', function (e) {
    if (state.mode !== 'flip') return;
    e.preventDefault();
    parent.postMessage({ cshowWheel: e.deltaX }, '*');
  }, { passive: false });

  // 手机触摸：翻页模式横向滑动 → 转交父窗口统一按整页翻
  var touchX = null, touchY = null, touchHoriz = false;
  document.addEventListener('touchstart', function (e) {
    if (state.mode !== 'flip') return;
    var t = e.touches[0];
    touchX = t.clientX; touchY = t.clientY; touchHoriz = false;
  }, { passive: true });
  document.addEventListener('touchmove', function (e) {
    if (state.mode !== 'flip' || touchX === null) return;
    var t = e.touches[0];
    var dx = t.clientX - touchX;
    var dy = t.clientY - touchY;
    touchX = t.clientX; touchY = t.clientY;
    if (!touchHoriz) {
      if (Math.abs(dx) < 6 && Math.abs(dy) < 6) return;
      if (Math.abs(dx) > Math.abs(dy)) touchHoriz = true;
      else { touchX = null; return; } // 纵向滑动：交给系统滚动
    }
    e.preventDefault();
    parent.postMessage({ cshowWheel: -dx }, '*'); // 手指左滑 = 下一页
  }, { passive: false });
  document.addEventListener('touchend', function () { touchX = null; });
  document.addEventListener('touchcancel', function () { touchX = null; });

  function boot() {
    applyStyle();
    if (state.mode === 'flip') buildWrap();
    scheduleMeasure();
    parent.postMessage({ cshowReady: { chapter: chapter, mode: state.mode } }, '*');
    setTimeout(scheduleMeasure, 250); // 图片/字体加载后二次校正
    setTimeout(scheduleMeasure, 600);
  }

  if (doc.readyState === 'loading') {
    doc.addEventListener('DOMContentLoaded', boot);
  } else {
    boot();
  }
})();
