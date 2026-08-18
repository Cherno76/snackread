import * as pdfjsLib from './vendor/pdfjs/pdf.min.mjs';
import { renderMarkdown } from './markdown.js';
pdfjsLib.GlobalWorkerOptions.workerSrc = './vendor/pdfjs/pdf.worker.min.mjs';

const { invoke, convertFileSrc } = window.__TAURI__.core;
// Android WebView 不支持自定义 scheme，wry 把 book:// 映射为 http://book.localhost；
// 页面内（iframe/script/img）引用必须用映射后的地址，否则 WebView 报“网页无法打开”
const BOOK_ORIGIN = /Android/i.test(navigator.userAgent)
  ? 'http://book.localhost'
  : 'book://localhost';
// 触摸设备（手机）：单击卡片直接进入，不做“先选中再进入”的两步操作
const IS_TOUCH = navigator.maxTouchPoints > 0 || /Android|iPhone|iPad|iPod/i.test(navigator.userAgent);

const sidebarEl = document.getElementById('sidebar');
const libTabsEl = document.getElementById('lib-tabs');
const libEyeEl = document.getElementById('lib-eye');
const previewEl = document.getElementById('preview');
const stripEl = document.getElementById('strip');
const versionEl = document.getElementById('version');
const statusbarEl = document.getElementById('statusbar');
const titlebarEl = document.getElementById('titlebar');
const titlebarTextEl = document.getElementById('titlebar-text');
const barTimeEl = document.getElementById('bar-time');
const barWifiEl = document.getElementById('bar-wifi');
const barCellEl = document.getElementById('bar-cell');
const batEl = document.getElementById('bar-battery');
const batFillEl = document.getElementById('bat-fill');
const batPctEl = document.getElementById('bat-pct');

// 标题栏右侧状态：时间 + 网络（Wi-Fi/蜂窝代次）+ 电池百分比
function titleBarStatus() {
  const d = new Date();
  barTimeEl.textContent = String(d.getHours()).padStart(2, '0') + ':' + String(d.getMinutes()).padStart(2, '0');
  let battery = -1, wifi = null;
  try {
    if (window.AndroidStatus) {
      const s = JSON.parse(window.AndroidStatus.getStatus());
      battery = s.battery;
      wifi = s.wifi;
    }
  } catch { /* 桥未就绪，稍后重试 */ }
  let cell = '';
  try {
    const ct = (navigator.connection && navigator.connection.effectiveType) || '';
    cell = ct === '4g' ? '4G' : (ct === '3g' ? '3G' : (ct === '2g' || ct === 'slow-2g' ? '2G' : ''));
  } catch { /* 忽略 */ }
  if (wifi === true) {
    barWifiEl.hidden = false;
    barCellEl.hidden = true;
  } else {
    barWifiEl.hidden = true;
    barCellEl.hidden = cell ? false : true;
    barCellEl.textContent = cell;
  }
  if (battery >= 0 && battery <= 100) {
    // 左右各留 1.5px 内边距：100% 时两侧边距一致，右边缘不溢出边框
    batFillEl.style.width = `calc(${battery}% - 3px)`;
    batPctEl.textContent = String(battery);
    batEl.classList.toggle('low', battery <= 20);
    // 电量条覆盖到文字时用白字保证对比度
    batPctEl.style.color = battery > 55 ? '#fff' : '';
    batEl.hidden = false;
  } else {
    batEl.hidden = true;
  }
}
titleBarStatus();
setInterval(titleBarStatus, 30000);
if (navigator.connection) {
  navigator.connection.addEventListener('change', titleBarStatus);
}
const progressEl = document.getElementById('progress');
const readingEl = document.getElementById('reading');
const modeTabEl = document.getElementById('mode-tab');
const modeScrollBtn = document.getElementById('mode-scroll');
const modeFlipBtn = document.getElementById('mode-flip');
const pageModeEl = document.getElementById('page-mode');
const pageSingleBtn = document.getElementById('page-single');
const pageDoubleBtn = document.getElementById('page-double');
const themeBtnEl = document.getElementById('theme-btn');
const readProgressEl = document.getElementById('read-progress');
const readProgressFillEl = document.getElementById('read-progress-fill');
const gridStatsEl = document.getElementById('grid-stats');
const libStatsEl = document.getElementById('lib-stats');
const tagBarEl = document.getElementById('tag-bar');
const metaDialogEl = document.getElementById('meta-dialog');
const metaDialogCloseEl = document.getElementById('meta-dialog-close');
const metaTitleEl = document.getElementById('meta-title');
const metaAuthorEl = document.getElementById('meta-author');
const metaStarsEl = document.getElementById('meta-stars');
const metaRatingNoteEl = document.getElementById('meta-rating-note');
const metaTagsEl = document.getElementById('meta-tags');
const metaSmartEl = document.getElementById('meta-smart');
const metaSmartStatusEl = document.getElementById('meta-smart-status');
const metaFixTocEl = document.getElementById('meta-fix-toc');
const metaFixStatusEl = document.getElementById('meta-fix-status');
const metaCancelEl = document.getElementById('meta-cancel');
const metaSaveEl = document.getElementById('meta-save');
const metaCoverEl = document.getElementById('meta-cover');
const metaCoverPreviewEl = document.getElementById('meta-cover-preview');
const metaCoverFileEl = document.getElementById('meta-cover-file');
const metaCoverPickEl = document.getElementById('meta-cover-pick');
const metaCoverRemoveEl = document.getElementById('meta-cover-remove');
const metaCoverStatusEl = document.getElementById('meta-cover-status');
const statsDialogEl = document.getElementById('stats-dialog');
const statsDialogCloseEl = document.getElementById('stats-dialog-close');
const statsSummaryEl = document.getElementById('stats-summary');
const statsRecentEl = document.getElementById('stats-recent');
const nextVolDialogEl = document.getElementById('next-vol-dialog');
const nextVolCoverEl = document.getElementById('next-vol-cover');
const nextVolTimeEl = document.getElementById('next-vol-time');
const nextVolNameEl = document.getElementById('next-vol-name');
const nextVolContinueEl = document.getElementById('next-vol-continue');
const nextVolCancelEl = document.getElementById('next-vol-cancel');
const nextVolExitEl = document.getElementById('next-vol-exit');
const metaNoteEl = document.getElementById('meta-note');
const hoverTipEl = document.getElementById('hover-tip');
const tocBtnEl = document.getElementById('toc-btn');
const readerBackEl = document.getElementById('reader-back');
const fontMinusBtn = document.getElementById('font-minus');
const fontPlusBtn = document.getElementById('font-plus');
const fontFamilyBtn = document.getElementById('font-family');
const tocPanelEl = document.getElementById('toc-panel');
const tocListEl = document.getElementById('toc-list');
const tocPanelTitleEl = document.getElementById('toc-panel-title');
const pageNavBtnEl = document.getElementById('page-nav-btn');
const tocBackdropEl = document.getElementById('toc-backdrop');
const tocCloseBtn = document.getElementById('toc-close');

let cwd = '';
let entries = [];    // 当前目录全部条目（与侧栏顺序一致）
let images = [];     // 当前目录的图片路径（子集）
let pages = [];      // 当前条漫的页：{path, name, kind:'img'|'pdf'}
let favorites = [];  // 个人收藏的目录路径
let stripKind = 'images';
let pdfDoc = null;
let currentIdx = -1; // 条漫中当前页下标
let listSel = 0;     // 列表模式键盘光标
let focus = 'list';  // 'list' 列表模式 | 'strip' 条漫模式
let stripReturn = null; // 图片分卷条漫退出后要返回的电子书主文件夹 {parentDir, ebookPath}
let flipOn = false;     // 翻页模式
let rtl = false;        // 右开（← 前进）
let flipBookDir = null; // 当前书目录（阅读设置存取）
let flipVolumeKey = null; // 散装书时按文件路径隔离阅读设置
let flipSettingsPromise = null;
let flipEpub = null;            // EPUB 分页数据 {paths, chapter_offsets}
let flipEpubChapter = 0;        // 切到翻页前的章节
let pendingVol = null;          // 进入分卷时待恢复的位置 {page, mode}
let flipWheelAccum = 0;         // 翻页模式触控板横向滑动累积
let flipCooldown = 0;           // 翻页冷却截止时间戳
let doublePage = false;         // 翻页模式双页
let flipAnchor = 0;             // 翻页当前位置锚点（布局无关）
let stripBuilt = false;
let stripPdfPath = null; // 当前条漫构建自哪个 PDF（用于切换检测）
let stripEpubPath = null; // 当前条漫构建自哪个 EPUB
let epubMeta = null;      // 当前 EPUB：{base_dir, spine, title}
let pendingPdfPage = 0; // 进入 PDF 条漫时恢复的页码
let pendingEpubPage = 0; // 进入 EPUB 条漫时恢复的章节序号
let epubBookToken = '';   // 本次打开的书的随机 token（防缓存串书）
// ---- 文字 EPUB 阅读状态 ----
let textBook = false;            // 当前 EPUB 是否为文字书
let textChapterPages = [];       // 每章列数（分页模式，0=未测量）
let textChapterStart = [];       // 每章起始全局列偏移
let textTotalPages = 0;          // 全书总列数
let textNavBuilt = 0;            // 底部导航条上次构建的页数（翻页模式）
let textChapterLengths = [];     // 每章正文字符数（用于阅读百分比）
let textTotalChars = 0;          // 全书总字符数
let textCol = 0;                 // 当前全局列下标（分页模式，派生展示用）
let textCurChapter = 0;          // 当前章节（分页模式跨章翻页的权威位置）
let textCurColInChapter = 0;     // 当前章节内列下标
let textPendingChapter = null;   // 进入分页时待定位的章节（几何就绪后定位）
let textWaitChapter = null;      // 字号/字体变更后等待重定位的章节
let textPendingFrac = 0;         // 进入分页时待定位的章节内进度（0..1）
// 文字书按需加载：只加载当前章 ± TEXT_WINDOW，跨章翻页时再按需加载/测量
let textLoaded = new Set();      // 已设置 src 的章节（加载中或已就绪）
let textLoadPromises = new Map();// ch -> Promise（几何就绪或超时）
let textGeomWaiters = new Map(); // ch -> [{resolve, timer}]
let textCharsPerPage = 0;        // 每页字符数参考（由已测量章节推算，用于估算未加载章节页数）
let pendingAnchor = null;        // 目录跳转的待定位锚点 {chapter, anchor}（等目标章就绪后发给 reader）
let pendingAnchorCol = null;     // reader 已回报锚点列号，但目标章几何尚未上报时的待定位列
let textPendingAnimate = false;  // 跨章翻页：待定位完成后是否用滑动过渡（用户翻页=true）
let pendingScrollRestore = null; // 翻页→滚动时待恢复的滚动位置 {chapter, frac}
let readerFontSize = 16;         // 正文字号（px）
let readerFontFamily = 'system'; // system | serif | sans
let readTimes = {};       // 各书籍累计阅读时长（秒）内存缓存：{ bookPath: seconds }（以 .cshow 为准）
let readingStartAt = 0;   // 本次阅读开始的毫秒时间戳
let bookMetaMap = {};     // path -> {title, author, rating, tags}
let allTags = [];         // 当前书库全部标签
let tagRefCount = new Map(); // 标签 → 被多少本书引用
let activeTags = new Set(); // 标签筛选选中的标签
let metaDialogBook = null;  // 元数据对话框当前编辑的书
let metaDialogRating = 0;   // 元数据对话框当前评分（0..5）
let metaPendingCover = null; // 待保存的自定义封面：{name, data} | 'remove' | null
let volCache = new Map();   // dir -> ebook_volumes 结果（网格渲染缓存）

// 一次性把历史 localStorage 数据（阅读时长、阅读背景主题）迁到后端，之后彻底不再读写 localStorage
async function migrateLegacyStorage() {
  try {
    if (await invoke('is_migrated')) return;
  } catch { /* 拿不到标记则重新执行迁移 */ }

  // 阅读时长
  let times = {};
  try { times = JSON.parse(localStorage.getItem('cshow.readTimes') || '{}') || {}; } catch { times = {}; }
  const entries = Object.entries(times).filter(([, v]) => Number(v) > 0);
  for (const [path, sec] of entries) {
    try { await invoke('add_reading_time', { path, seconds: Math.round(Number(sec)) }); } catch { /* 忽略 */ }
  }
  // 阅读背景主题
  let theme = null;
  try { theme = localStorage.getItem('cshow.readerTheme'); } catch { /* 忽略 */ }
  if (theme) {
    try { await invoke('set_reader_theme', { theme }); } catch { /* 忽略 */ }
  }
  // 正文字号/字体
  let fs = 16;
  try { fs = parseInt(localStorage.getItem('cshow.readerFontSize') || '16', 10) || 16; } catch { /* 忽略 */ }
  let ff = 'system';
  try { ff = localStorage.getItem('cshow.readerFontFamily') || 'system'; } catch { /* 忽略 */ }
  try { await invoke('set_reader_font', { size: fs, family: ff }); } catch { /* 忽略 */ }
  // 清理旧键并标记完成
  try { localStorage.removeItem('cshow.readTimes'); } catch { /* 忽略 */ }
  try { localStorage.removeItem('cshow.readerTheme'); } catch { /* 忽略 */ }
  try { localStorage.removeItem('cshow.readerFontSize'); } catch { /* 忽略 */ }
  try { localStorage.removeItem('cshow.readerFontFamily'); } catch { /* 忽略 */ }
  try { await invoke('mark_migrated'); } catch { /* 忽略 */ }
}

// 读取阅读背景主题（后端配置目录）
async function loadReaderTheme() {
  let theme = 'light';
  try { theme = await invoke('get_reader_theme'); } catch { /* 忽略 */ }
  if (!READER_THEMES.includes(theme)) theme = 'light';
  return theme;
}
function fmtDuration(sec) {
  sec = Math.max(0, Math.floor(sec));
  const h = Math.floor(sec / 3600);
  const m = Math.floor((sec % 3600) / 60);
  const s = sec % 60;
  if (h > 0) return `${h} 小时 ${m} 分`;
  if (m > 0) return `${m} 分 ${s} 秒`;
  return `${s} 秒`;
}

function escapeHtml(s) {
  return s.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;').replace(/"/g, '&quot;');
}
function baseName(p) {
  return p.split('/').pop();
}

// Lucide 的 bookmark 图标（书签徽标）
function bookmarkIconEl() {
  const NS = 'http://www.w3.org/2000/svg';
  const svg = document.createElementNS(NS, 'svg');
  svg.setAttribute('viewBox', '0 0 24 24');
  svg.setAttribute('width', '14');
  svg.setAttribute('height', '14');
  svg.setAttribute('fill', 'currentColor');
  svg.setAttribute('stroke', 'currentColor');
  svg.setAttribute('stroke-width', '2');
  svg.setAttribute('stroke-linecap', 'round');
  svg.setAttribute('stroke-linejoin', 'round');
  svg.setAttribute('aria-hidden', 'true');
  const p = document.createElementNS(NS, 'path');
  p.setAttribute('d', 'm19 21-7-4-7 4V5a2 2 0 0 1 2-2h10a2 2 0 0 1 2 2v16z');
  svg.appendChild(p);
  return svg;
}

// Lucide 的 check（对勾）图标
function checkIconEl() {
  const NS = 'http://www.w3.org/2000/svg';
  const svg = document.createElementNS(NS, 'svg');
  svg.setAttribute('viewBox', '0 0 24 24');
  svg.setAttribute('width', '14');
  svg.setAttribute('height', '14');
  svg.setAttribute('fill', 'none');
  svg.setAttribute('stroke', 'currentColor');
  svg.setAttribute('stroke-width', '3');
  svg.setAttribute('stroke-linecap', 'round');
  svg.setAttribute('stroke-linejoin', 'round');
  svg.setAttribute('aria-hidden', 'true');
  const p = document.createElementNS(NS, 'path');
  p.setAttribute('d', 'M20 6 9 17l-5-5');
  svg.appendChild(p);
  return svg;
}

// Lucide 的 eye / eye-off 图标
function eyeIconEl(off) {
  const NS = 'http://www.w3.org/2000/svg';
  const svg = document.createElementNS(NS, 'svg');
  svg.setAttribute('viewBox', '0 0 24 24');
  svg.setAttribute('width', '15');
  svg.setAttribute('height', '15');
  svg.setAttribute('fill', 'none');
  svg.setAttribute('stroke', 'currentColor');
  svg.setAttribute('stroke-width', '2');
  svg.setAttribute('stroke-linecap', 'round');
  svg.setAttribute('stroke-linejoin', 'round');
  svg.setAttribute('aria-hidden', 'true');
  if (off) {
    const p1 = document.createElementNS(NS, 'path');
    p1.setAttribute('d', 'M9.88 9.88a3 3 0 1 0 4.24 4.24');
    const p2 = document.createElementNS(NS, 'path');
    p2.setAttribute('d', 'M10.73 5.08A10.43 10.43 0 0 1 12 5c7 0 10 7 10 7a13.16 13.16 0 0 1-1.67 2.68');
    const p3 = document.createElementNS(NS, 'path');
    p3.setAttribute('d', 'M6.61 6.61A13.526 13.526 0 0 0 2 12s3 7 10 7a9.74 9.74 0 0 0 5.39-1.61');
    const l1 = document.createElementNS(NS, 'line');
    l1.setAttribute('x1', '2'); l1.setAttribute('x2', '22');
    l1.setAttribute('y1', '2'); l1.setAttribute('y2', '22');
    svg.appendChild(p1); svg.appendChild(p2); svg.appendChild(p3); svg.appendChild(l1);
  } else {
    const p1 = document.createElementNS(NS, 'path');
    p1.setAttribute('d', 'M2 12s3-7 10-7 10 7 10 7-3 7-10 7-10-7-10-7Z');
    const c1 = document.createElementNS(NS, 'circle');
    c1.setAttribute('cx', '12'); c1.setAttribute('cy', '12'); c1.setAttribute('r', '3');
    svg.appendChild(p1); svg.appendChild(c1);
  }
  return svg;
}

// Lucide 的 refresh-cw（刷新）图标
function refreshIconEl() {
  return makeSvg([
    { tag: 'path', attrs: { d: 'M21 12a9 9 0 1 1-9-9c2.52 0 4.93 1 6.74 2.74L21 8' } },
    { tag: 'path', attrs: { d: 'M21 3v5h-5' } },
  ]);
}

// Lucide 的 arrow-left（返回）图标
function arrowLeftIconEl() {
  return makeSvg([
    { tag: 'path', attrs: { d: 'm12 19-7-7 7-7' } },
    { tag: 'path', attrs: { d: 'M19 12H5' } },
  ]);
}

// Lucide 的 rotate-ccw（重置）图标
function resetIconEl() {
  return makeSvg([
    { tag: 'path', attrs: { d: 'M3 12a9 9 0 1 0 9-9 9.75 9.75 0 0 0-6.74 2.74L3 8' } },
    { tag: 'path', attrs: { d: 'M3 3v5h5' } },
  ]);
}

// Lucide 的 settings（齿轮）图标
function gearIconEl() {
  const NS = 'http://www.w3.org/2000/svg';
  const svg = document.createElementNS(NS, 'svg');
  svg.setAttribute('viewBox', '0 0 24 24');
  svg.setAttribute('width', '14');
  svg.setAttribute('height', '14');
  svg.setAttribute('fill', 'none');
  svg.setAttribute('stroke', 'currentColor');
  svg.setAttribute('stroke-width', '2');
  svg.setAttribute('stroke-linecap', 'round');
  svg.setAttribute('stroke-linejoin', 'round');
  svg.setAttribute('aria-hidden', 'true');
  const c = document.createElementNS(NS, 'circle');
  c.setAttribute('cx', '12'); c.setAttribute('cy', '12'); c.setAttribute('r', '3');
  const p = document.createElementNS(NS, 'path');
  p.setAttribute('d', 'M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 1 1-2.83 2.83l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 1 1-4 0v-.09A1.65 1.65 0 0 0 9 19.4a1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 1 1-2.83-2.83l.06-.06a1.65 1.65 0 0 0 .33-1.82 1.65 1.65 0 0 0-1.51-1H3a2 2 0 1 1 0-4h.09A1.65 1.65 0 0 0 4.6 9a1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 1 1 2.83-2.83l.06.06a1.65 1.65 0 0 0 1.82.33H9a1.65 1.65 0 0 0 1-1.51V3a2 2 0 1 1 4 0v.09a1.65 1.65 0 0 0 1 1.51 1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 1 1 2.83 2.83l-.06.06a1.65 1.65 0 0 0-.33 1.82V9a1.65 1.65 0 0 0 1.51 1H21a2 2 0 1 1 0 4h-.09a1.65 1.65 0 0 0-1.51 1z');
  svg.appendChild(c);
  svg.appendChild(p);
  return svg;
}

function makeSvg(parts) {
  const NS = 'http://www.w3.org/2000/svg';
  const svg = document.createElementNS(NS, 'svg');
  svg.setAttribute('viewBox', '0 0 24 24');
  svg.setAttribute('width', '15');
  svg.setAttribute('height', '15');
  svg.setAttribute('fill', 'none');
  svg.setAttribute('stroke', 'currentColor');
  svg.setAttribute('stroke-width', '2');
  svg.setAttribute('stroke-linecap', 'round');
  svg.setAttribute('stroke-linejoin', 'round');
  svg.setAttribute('aria-hidden', 'true');
  for (const p of parts) {
    const el = document.createElementNS(NS, p.tag);
    for (const [k, v] of Object.entries(p.attrs || {})) el.setAttribute(k, v);
    svg.appendChild(el);
  }
  return svg;
}

function scrollIconEl() {
  return makeSvg([
    { tag: 'line', attrs: { x1: '8', x2: '21', y1: '6', y2: '6' } },
    { tag: 'line', attrs: { x1: '8', x2: '21', y1: '12', y2: '12' } },
    { tag: 'line', attrs: { x1: '8', x2: '21', y1: '18', y2: '18' } },
    { tag: 'line', attrs: { x1: '3', x2: '3.01', y1: '6', y2: '6' } },
    { tag: 'line', attrs: { x1: '3', x2: '3.01', y1: '12', y2: '12' } },
    { tag: 'line', attrs: { x1: '3', x2: '3.01', y1: '18', y2: '18' } },
  ]);
}

function bookOpenIconEl() {
  return makeSvg([
    { tag: 'path', attrs: { d: 'M2 3h6a4 4 0 0 1 4 4v14a3 3 0 0 0-3-3H2z' } },
    { tag: 'path', attrs: { d: 'M22 3h-6a4 4 0 0 0-4 4v14a3 3 0 0 1 3-3h7z' } },
  ]);
}

function rectIconEl() {
  return makeSvg([
    { tag: 'rect', attrs: { width: '20', height: '12', x: '2', y: '6', rx: '2' } },
  ]);
}

function columnsIconEl() {
  return makeSvg([
    { tag: 'rect', attrs: { width: '18', height: '18', x: '3', y: '3', rx: '2' } },
    { tag: 'path', attrs: { d: 'M12 3v18' } },
  ]);
}

// Lucide 的 star（评分星级）
function starIconEl(filled) {
  const svg = makeSvg([
    { tag: 'path', attrs: { d: 'M11.525 2.295a.53.53 0 0 1 .95 0l2.31 4.679a2.123 2.123 0 0 0 1.595 1.16l5.166.756a.53.53 0 0 1 .294.904l-3.736 3.638a2.123 2.123 0 0 0-.611 1.878l.882 5.14a.53.53 0 0 1-.771.56l-4.618-2.428a2.122 2.122 0 0 0-1.973 0L6.396 21.01a.53.53 0 0 1-.77-.56l.881-5.139a2.122 2.122 0 0 0-.611-1.879L2.16 9.795a.53.53 0 0 1 .294-.906l5.165-.755a2.122 2.122 0 0 0 1.597-1.16z' } },
  ]);
  svg.setAttribute('fill', filled ? 'currentColor' : 'none');
  return svg;
}

// Lucide 的 pencil（编辑元数据）
function pencilIconEl() {
  return makeSvg([
    { tag: 'path', attrs: { d: 'M21.174 6.812a1 1 0 0 0-3.986-3.987L3.842 16.174a2 2 0 0 0-.5.83l-1.321 4.352a.5.5 0 0 0 .623.622l4.353-1.32a2 2 0 0 0 .83-.497z' } },
  ]);
}

// Lucide 的 bar-chart（阅读统计）
function barChartIconEl() {
  return makeSvg([
    { tag: 'line', attrs: { x1: '12', x2: '12', y1: '20', y2: '10' } },
    { tag: 'line', attrs: { x1: '18', x2: '18', y1: '20', y2: '4' } },
    { tag: 'line', attrs: { x1: '6', x2: '6', y1: '20', y2: '16' } },
  ]);
}

// Lucide 的 library-big（书库）图标
function libraryBigIconEl() {
  return makeSvg([
    { tag: 'rect', attrs: { width: '8', height: '18', x: '3', y: '3', rx: '1' } },
    { tag: 'path', attrs: { d: 'M7 3v18' } },
    { tag: 'path', attrs: { d: 'M20.4 18.9c.2.5-.1 1.1-.6 1.3l-1.9.7c-.5.2-1.1-.1-1.3-.6L11.1 5.1c-.2-.5.1-1.1.6-1.3l1.9-.7c.5-.2 1.1.1 1.3.6Z' } },
  ]);
}

// 可选书库图标（Lucide）
const LIBRARY_ICONS = ['book-user', 'book-image', 'book-text', 'book-marked', 'book-heart', 'book-lock', 'square-library'];

function libraryIconEl(key) {
  switch (key) {
    case 'book-user':
      return makeSvg([
        { tag: 'path', attrs: { d: 'M15 13a3 3 0 1 0-6 0' } },
        { tag: 'path', attrs: { d: 'M4 19.5v-15A2.5 2.5 0 0 1 6.5 2H19a1 1 0 0 1 1 1v18a1 1 0 0 1-1 1H6.5a1 1 0 0 1 0-5H20' } },
        { tag: 'circle', attrs: { cx: '12', cy: '8', r: '2' } },
      ]);
    case 'book-image':
      return makeSvg([
        { tag: 'path', attrs: { d: 'm20 13.7-2.1-2.1a2 2 0 0 0-2.8 0L9.7 17' } },
        { tag: 'path', attrs: { d: 'M4 19.5v-15A2.5 2.5 0 0 1 6.5 2H19a1 1 0 0 1 1 1v18a1 1 0 0 1-1 1H6.5a1 1 0 0 1 0-5H20' } },
        { tag: 'circle', attrs: { cx: '10', cy: '8', r: '2' } },
      ]);
    case 'book-text':
      return makeSvg([
        { tag: 'path', attrs: { d: 'M4 19.5v-15A2.5 2.5 0 0 1 6.5 2H19a1 1 0 0 1 1 1v18a1 1 0 0 1-1 1H6.5a1 1 0 0 1 0-5H20' } },
        { tag: 'path', attrs: { d: 'M8 11h8' } },
        { tag: 'path', attrs: { d: 'M8 7h6' } },
      ]);
    case 'book-marked':
      return makeSvg([
        { tag: 'path', attrs: { d: 'M10 2v8l3-3 3 3V2' } },
        { tag: 'path', attrs: { d: 'M4 19.5v-15A2.5 2.5 0 0 1 6.5 2H19a1 1 0 0 1 1 1v18a1 1 0 0 1-1 1H6.5a1 1 0 0 1 0-5H20' } },
      ]);
    case 'book-heart':
      return makeSvg([
        { tag: 'path', attrs: { d: 'M4 19.5v-15A2.5 2.5 0 0 1 6.5 2H19a1 1 0 0 1 1 1v18a1 1 0 0 1-1 1H6.5a1 1 0 0 1 0-5H20' } },
        { tag: 'path', attrs: { d: 'M8.62 9.8A2.25 2.25 0 1 1 12 6.836a2.25 2.25 0 1 1 3.38 2.966l-2.626 2.856a.998.998 0 0 1-1.507 0z' } },
      ]);
    case 'book-lock':
      return makeSvg([
        { tag: 'path', attrs: { d: 'M18 6V4a2 2 0 1 0-4 0v2' } },
        { tag: 'path', attrs: { d: 'M20 15v6a1 1 0 0 1-1 1H6.5a1 1 0 0 1 0-5H20' } },
        { tag: 'path', attrs: { d: 'M4 19.5v-15A2.5 2.5 0 0 1 6.5 2H10' } },
        { tag: 'rect', attrs: { x: '12', y: '6', width: '8', height: '5', rx: '1' } },
      ]);
    case 'square-library':
      return makeSvg([
        { tag: 'rect', attrs: { width: '18', height: '18', x: '3', y: '3', rx: '2' } },
        { tag: 'path', attrs: { d: 'M7 7v10' } },
        { tag: 'path', attrs: { d: 'M11 7v10' } },
        { tag: 'path', attrs: { d: 'm15 7 2 10' } },
      ]);
    default:
      return libraryBigIconEl();
  }
}

// 阅读背景主题：白 / 羊皮纸 / 暗
const READER_THEMES = ['light', 'sepia', 'dark'];
const READER_THEME_LABEL = { light: '白色', sepia: '羊皮纸', dark: '暗色' };
let readerTheme = 'light';

function themeIconEl() {
  return makeSvg([
    { tag: 'circle', attrs: { cx: '12', cy: '12', r: '9' } },
    { tag: 'path', attrs: { d: 'M12 3a9 9 0 0 1 0 18z' } },
  ]);
}

function applyReaderTheme(theme, persist = true) {
  readerTheme = theme;
  stripEl.classList.remove('theme-sepia', 'theme-dark');
  if (theme === 'sepia') stripEl.classList.add('theme-sepia');
  else if (theme === 'dark') stripEl.classList.add('theme-dark');
  if (persist) {
    if (flipBookDir) {
      // 单书记忆：阅读背景随书保存（与阅读模式/字号同一 scope）
      invoke('write_book_settings', {
        ebookDir: flipBookDir,
        volume: flipVolumeKey,
        readMode: flipOn ? 'flip' : 'scroll',
        rtl,
        doublePage,
        fontSize: readerFontSize,
        fontFamily: readerFontFamily,
        theme,
      }).catch(() => {});
    } else {
      // 未进入阅读：写全局默认
      invoke('set_reader_theme', { theme }).catch(() => {});
    }
  }
  themeBtnEl.title = '阅读背景：' + READER_THEME_LABEL[theme] + '（点击切换）';
  if (textBook) broadcastReaderCfg(); // 主题作用于 iframe 内正文
}

function cycleReaderTheme() {
  const i = READER_THEMES.indexOf(readerTheme);
  applyReaderTheme(READER_THEMES[(i + 1) % READER_THEMES.length]);
}

// ---- 文字 EPUB：字号/字体与排版/分页 ----

const FONT_FAMILIES = ['system', 'serif', 'sans'];
const FONT_FAMILY_LABEL = { system: '系统', serif: '宋体', sans: '黑体' };

async function loadReaderFont() {
  try {
    const f = await invoke('get_reader_font');
    if (f && typeof f.size === 'number' && f.size >= 10 && f.size <= 32) readerFontSize = f.size;
    if (f && FONT_FAMILIES.includes(f.family)) readerFontFamily = f.family;
  } catch { /* 忽略 */ }
}
function persistReaderFont() {
  if (flipBookDir) {
    // 单书记忆：写入本书设置（与阅读模式同一 scope）
    invoke('write_book_settings', {
      ebookDir: flipBookDir,
      volume: flipVolumeKey,
      readMode: flipOn ? 'flip' : 'scroll',
      rtl,
      doublePage,
      fontSize: readerFontSize,
      fontFamily: readerFontFamily,
    }).catch(() => {});
  } else {
    // 未进入阅读（如启动迁移）：写全局默认
    invoke('set_reader_font', { size: readerFontSize, family: readerFontFamily }).catch(() => {});
  }
}
function updateFontButtons() {
  fontFamilyBtn.textContent = FONT_FAMILY_LABEL[readerFontFamily] || '字体';
  fontFamilyBtn.title = '正文字体：' + FONT_FAMILY_LABEL[readerFontFamily] + '（点击切换）';
}

const READER_MARGIN = 28; // 正文四周边距（px），左右与上下一致

function textGap() { return doublePage ? 56 : 0; }
/// 文字书翻页视口宽：翻页模式下 strip 自身宽度被设为全书宽度（n×视口宽），
/// 不能用 stripEl.clientWidth，只能取窗口宽（阅读区占满窗口）。
function textViewportW() {
  return Math.max(1, window.innerWidth || stripEl.clientWidth || 800);
}
function textPageW() {
  const W = textViewportW();
  const g = textGap();
  const contentW = Math.max(200, W - 2 * READER_MARGIN);
  // 双页：精确取 (contentW-gap)/2（可为 .5px），否则 Math.round 四舍五入会让两列合计比容器多 1px，
  // 触发 CSS 多列「只放得下一列」而退化成整页单栏
  return doublePage ? Math.max(160, (contentW - g) / 2) : Math.round(contentW);
}
function textPageH() { return Math.max(200, stripEl.clientHeight || 600); }

function readerCfg() {
  return {
    theme: readerTheme,
    fs: readerFontSize,
    ff: readerFontFamily,
    lh: 1.7,
    mg: READER_MARGIN,
    mode: flipOn ? 'flip' : 'scroll',
    pageW: textPageW(),
    pageH: textPageH(),
    gap: textGap(),
  };
}
function sendReaderCfgTo(frame) {
  try { frame.contentWindow.postMessage({ cshow: 'reader', type: 'set', cfg: readerCfg() }, '*'); } catch { /* 忽略 */ }
}
function broadcastReaderCfg() {
  for (const frame of stripEl.querySelectorAll('.page.epub iframe')) sendReaderCfgTo(frame);
}

const TEXT_WINDOW = 1;           // 前后缓存章节数（含当前章共 2*TEXT_WINDOW+1）
const TEXT_DEFAULT_CHARS_PER_PAGE = 1600; // 无测量参考时单页的粗略字符数估算
const HOLDER_WINDOW = 30;        // 占位 div 只保留当前章前后各 N 章（虚拟化，避免大书全量 DOM/布局）

let textChapterHeights = [];     // 滚动模式每章内容高度（cshowH 实测，未测默认 120）
let textChapterTop = [];         // 高度前缀和：textChapterTop[i] = 第 i 章顶部偏移
let textHolderLo = 0, textHolderHi = -1; // 当前占位 div 窗口 [lo, hi]
let textSpacerBefore = null, textSpacerAfter = null;
let textIo = null;               // 章节懒加载观察器（只观察窗口内的占位 div）
let textHolderScrollRaf = 0;     // 滚动模式窗口跟随的 rAF 句柄

function initTextHeightTable() {
  const n = epubMeta ? epubMeta.spine.length : 0;
  textChapterHeights = new Array(n).fill(120);
  rebuildTextChapterTop();
}

function rebuildTextChapterTop() {
  const n = textChapterHeights.length;
  textChapterTop = new Array(n + 1);
  textChapterTop[0] = 0;
  for (let i = 0; i < n; i++) textChapterTop[i + 1] = textChapterTop[i] + (textChapterHeights[i] || 120);
}

function ensureTextSpacers() {
  if (!textSpacerBefore) {
    textSpacerBefore = document.createElement('div');
    textSpacerBefore.className = 'txt-spacer before';
    stripEl.insertBefore(textSpacerBefore, stripEl.firstChild);
  }
  if (!textSpacerAfter) {
    textSpacerAfter = document.createElement('div');
    textSpacerAfter.className = 'txt-spacer after';
    stripEl.appendChild(textSpacerAfter);
  }
}

/// 用首尾两个占位 spacer 撑出窗口外章节的空间：
/// 翻页模式每章固定占视口宽；滚动模式按每章高度表（未测 120）累计。
function updateTextSpacers() {
  if (!textBook || !epubMeta || !textSpacerBefore || !textSpacerAfter) return;
  const n = epubMeta.spine.length;
  if (textHolderHi < textHolderLo || n === 0) return;
  if (flipOn) {
    // 翻页模式：窗口 holder 绝对定位、strip 自身撑出全书宽度，不用 spacer
    textSpacerBefore.style.display = 'none';
    textSpacerAfter.style.display = 'none';
  } else {
    textSpacerBefore.style.display = '';
    textSpacerAfter.style.display = '';
    textSpacerBefore.style.height = ((textChapterTop[textHolderLo] || 0)) + 'px';
    textSpacerAfter.style.height = ((textChapterTop[n] || 0) - (textChapterTop[textHolderHi + 1] || 0)) + 'px';
  }
}

/// 翻页模式：按视口宽重排窗口内 holder 的绝对位置（resize 后调用）
function layoutFlipHolders(lo, hi, W) {
  for (let i = lo; i <= hi; i++) {
    const holder = stripEl.querySelector('.page.epub[data-chapter="' + i + '"]');
    if (!holder) continue;
    holder.style.position = 'absolute';
    holder.style.top = '0';
    holder.style.left = (i * W) + 'px';
    holder.style.width = W + 'px';
    holder.style.height = '100%';
  }
}

/// 占位 div 虚拟化：只保留当前章 ±HOLDER_WINDOW，其余移除；翻到远处时按需补齐。
function syncTextHolders(center) {
  if (!textBook || !epubMeta) return;
  ensureTextSpacers();
  const n = epubMeta.spine.length;
  const lo = Math.max(0, center - HOLDER_WINDOW);
  const hi = Math.min(n - 1, center + HOLDER_WINDOW);
  const W = textViewportW();
  if (lo === textHolderLo && hi === textHolderHi) {
    if (flipOn) {
      layoutFlipHolders(lo, hi, W);
    }
    updateTextSpacers(); // 窗口未变也刷新 spacer（resize/模式切换后视口宽变化）
    return;
  }
  for (const h of Array.from(stripEl.querySelectorAll('.page.epub'))) {
    const ch = parseInt(h.dataset.chapter, 10);
    if (ch < lo || ch > hi) h.remove();
  }
  textHolderLo = lo;
  textHolderHi = hi;
  for (let i = lo; i <= hi; i++) {
    if (stripEl.querySelector('.page.epub[data-chapter="' + i + '"]')) continue;
    const holder = document.createElement('div');
    holder.className = 'page epub';
    holder.dataset.chapter = String(i);
    if (flipOn) {
      // 翻页模式：绝对定位，位置 = 章节下标 × 视口宽
      holder.style.position = 'absolute';
      holder.style.top = '0';
      holder.style.left = (i * W) + 'px';
      holder.style.width = W + 'px';
      holder.style.height = '100%';
      stripEl.appendChild(holder);
    } else {
      if (textSpacerAfter) stripEl.insertBefore(holder, textSpacerAfter);
      else stripEl.appendChild(holder);
      // 清掉可能残留的翻页绝对定位样式
      holder.style.position = '';
      holder.style.left = '';
      holder.style.width = '';
      holder.style.height = '';
    }
    if (textIo) textIo.observe(holder);
  }
  updateTextSpacers();
}

/// 取章节占位 div；不在窗口内时先同步窗口再取。
function holderFor(ch) {
  if (!epubMeta) return null;
  let h = stripEl.querySelector('.page.epub[data-chapter="' + ch + '"]');
  if (!h && textBook) {
    syncTextHolders(ch);
    h = stripEl.querySelector('.page.epub[data-chapter="' + ch + '"]');
  }
  return h;
}

function textFrame(ch) {
  const holder = holderFor(ch);
  if (!holder) return null;
  let frame = holder.querySelector('iframe');
  if (!frame) {
    frame = document.createElement('iframe');
    frame.scrolling = 'no';
    frame.dataset.chapter = String(ch);
    let href = epubMeta.spine[ch];
    try { href = decodeURIComponent(href); } catch { /* 保持原样 */ }
    const url = BOOK_ORIGIN + '/' + href.split('/').map(encodeURIComponent).join('/');
    let params = 'b=' + epubBookToken + '&c=' + ch;
    if (textBook) {
      params += '&t=' + encodeURIComponent(readerTheme) + '&fs=' + readerFontSize +
        '&ff=' + encodeURIComponent(readerFontFamily) + '&m=scroll';
    }
    frame.dataset.src = url + '?' + params;
    holder.appendChild(frame);
    pruneTextFrames(ch);
  }
  return frame;
}

// WKWebView 对单页 iframe 数量有限制（约 1000 个后不再加载内容，表现为白页）。
// 长书（>1000 章）只保留阅读窗口附近的 iframe，其余移除，避免后段章节白屏。
function pruneTextFrames(keep) {
  const LIMIT = 800;
  const all = () => stripEl.querySelectorAll('.page.epub iframe');
  if (all().length <= LIMIT) return;
  for (const f of all()) {
    if (all().length <= LIMIT) break;
    const holder = f.closest('.page.epub');
    if (!holder) continue;
    const ch = parseInt(holder.dataset.chapter, 10) || 0;
    if (typeof keep === 'number' && Math.abs(ch - keep) <= TEXT_WINDOW) continue;
    if (!flipOn) {
      // 滚动模式：记录已测量高度，避免移除 iframe 后布局塌陷
      const h = f.style.height;
      if (h) holder.style.minHeight = h;
    }
    f.remove();
    textLoaded.delete(ch);
    textLoadPromises.delete(ch);
    const arr = textGeomWaiters.get(ch);
    if (arr) {
      textGeomWaiters.delete(ch);
      for (const w of arr) { clearTimeout(w.timer); w.resolve(); }
    }
  }
}

/// 把滚动模式下已懒加载的章节并入 textLoaded（翻页窗口据此回收/保留）
function syncTextLoadedSet() {
  textLoaded.clear();
  for (const holder of stripEl.querySelectorAll('.page.epub')) {
    const f = holder.querySelector('iframe');
    const src = f && f.src;
    if (src && src !== 'about:blank') textLoaded.add(parseInt(holder.dataset.chapter, 10) || 0);
  }
}

/// 章节页数：已测量用实测值；未加载章节按字符量估算（避免为导航/恢复加载全部分卷）
function chapterPageCount(ch) {
  const m = textChapterPages[ch];
  if (m > 0) return m;
  const chars = textChapterLengths[ch] || 1;
  // 参考值给下限（200 字符/页），估算页数给上限（200 页/章），
  // 防止封面/制作信息等短页被测量后让估算页数爆炸
  const ref = Math.max(200, textCharsPerPage)
    || (doublePage ? Math.round(TEXT_DEFAULT_CHARS_PER_PAGE / 2) : TEXT_DEFAULT_CHARS_PER_PAGE);
  return Math.max(1, Math.min(200, Math.round(chars / ref)));
}

/// 由已测量章节推算「每页字符数」，作为未加载章节页数估算参考。
/// 只统计正常正文章节（>=500 字符）：封面/制作信息/简介等短页不参与，
/// 否则 5 字符的封面页被分成几页会让参考值坍缩到个位数，导致估算页数爆炸。
function updateCharsPerPageRef() {
  let chars = 0, pages = 0;
  for (let i = 0; i < textChapterPages.length; i++) {
    const p = textChapterPages[i];
    const len = textChapterLengths[i] || 0;
    if (p > 0 && len >= 500) { chars += len; pages += p; }
  }
  textCharsPerPage = pages > 0 ? chars / pages : 0;
}

/// 加载章节 iframe；返回的 promise 在该章几何（cshowGeom）就绪或 8s 超时时 resolve
function loadTextChapter(ch) {
  const n = epubMeta ? epubMeta.spine.length : 0;
  if (ch < 0 || ch >= n) return Promise.resolve();
  if (textLoadPromises.has(ch)) return textLoadPromises.get(ch);
  const frame = textFrame(ch);
  if (!frame) return Promise.resolve();
  if (textLoaded.has(ch) && (textChapterPages[ch] || 0) > 0) return Promise.resolve();
  if (!frame.getAttribute('src') || frame.src === 'about:blank') {
    frame.src = frame.dataset.src;
  }
  textLoaded.add(ch);
  const p = new Promise((resolve) => {
    // 几何通常在 100~300ms 内到达；700ms 未到则主动请求一次测量并再等 400ms，
    // 仍不到就按估算显示（几何晚到时由 onTextGeom 自动校正）
    const timer = setTimeout(() => {
      const f = stripEl.querySelector(`.page.epub[data-chapter="${ch}"] iframe`);
      if (f && f.contentWindow) {
        try { f.contentWindow.postMessage({ cshow: 'reader', type: 'measure' }, '*'); } catch { /* 忽略 */ }
      }
      const retry = setTimeout(() => {
        const arr = textGeomWaiters.get(ch);
        if (arr) textGeomWaiters.set(ch, arr.filter(w => w.timer !== timer));
        resolve();
      }, 400);
      const arr = textGeomWaiters.get(ch);
      if (arr) {
        for (const w of arr) {
          if (w.timer === timer) w.retry = retry;
        }
      }
    }, 700);
    const arr = textGeomWaiters.get(ch) || [];
    arr.push({ resolve, timer, retry: null });
    textGeomWaiters.set(ch, arr);
  });
  textLoadPromises.set(ch, p);
  return p;
}

/// 卸载远离当前阅读窗口的章节（保留已测量页数，回看时按需重载）
function unloadTextChapter(ch) {
  if (!textLoaded.has(ch)) return;
  const holder = stripEl.querySelector(`.page.epub[data-chapter="${ch}"]`);
  const frame = holder && holder.querySelector('iframe');
  if (frame) frame.remove(); // 直接移除 iframe，避免 WKWebView 大量 iframe 白屏
  textLoaded.delete(ch);
  const arr = textGeomWaiters.get(ch);
  if (arr) {
    textGeomWaiters.delete(ch);
    for (const w of arr) { clearTimeout(w.timer); if (w.retry) clearTimeout(w.retry); w.resolve(); }
  }
  textLoadPromises.delete(ch);
}

/// 只保留当前章 ± TEXT_WINDOW 加载，其余卸载
function reconcileTextWindow(center) {
  if (!flipOn || !textBook || !epubMeta) return;
  const n = epubMeta.spine.length;
  const lo = Math.max(0, center - TEXT_WINDOW);
  const hi = Math.min(n - 1, center + TEXT_WINDOW);
  for (let i = lo; i <= hi; i++) loadTextChapter(i);
  for (const ch of Array.from(textLoaded)) {
    if (ch < lo || ch > hi) unloadTextChapter(ch);
  }
}

/// 待定位章节几何就绪后按保存的章节内进度精确定位
function finishPendingLocate(ch) {
  if (textPendingChapter !== ch) return;
  const pages = textChapterPages[ch] || 0;
  if (pages <= 0) return; // 仍未测量：继续等 onTextGeom
  const frac = textPendingFrac;
  const animate = textPendingAnimate;
  textPendingChapter = null;
  textShowChapter(ch, Math.round(frac * Math.max(0, pages - 1)), animate);
}

/// 定位到某章的章节内进度；未加载/未测量时先加载，几何就绪后定位
function textGotoChapterFrac(ch, frac, animate) {
  const n = epubMeta ? epubMeta.spine.length : 0;
  if (n === 0) return;
  ch = Math.max(0, Math.min(ch, n - 1));
  frac = Math.max(0, Math.min(1, frac));
  const pages = textChapterPages[ch] || 0;
  if (pages > 0 && textLoaded.has(ch)) {
    textPendingChapter = null;
    textShowChapter(ch, Math.round(frac * Math.max(0, pages - 1)), animate);
    return;
  }
  // 目标章未测量：先用估算位置立即切换（跨章翻页不被几何等待阻塞），
  // 几何就绪后由 onTextGeom 用 textPendingChapter 校正到精确列（瞬移，不再二次动画）。
  const est = chapterPageCount(ch);
  textPendingChapter = ch;
  textPendingFrac = frac;
  textPendingAnimate = false;
  textWaitChapter = null;
  textShowChapter(ch, Math.round(frac * Math.max(0, est - 1)), animate);
  loadTextChapter(ch);
  reconcileTextWindow(ch);
}

function rebuildTextOffsets() {
  let acc = 0;
  textChapterStart = [];
  for (let i = 0; i < textChapterPages.length; i++) {
    textChapterStart[i] = acc;
    acc += chapterPageCount(i);
  }
  textTotalPages = acc;
}
function textChapterOf(col) {
  let ch = 0;
  for (let i = 0; i < textChapterStart.length; i++) {
    if (textChapterStart[i] <= col) ch = i; else break;
  }
  return ch;
}

function currentTextCol() {
  return (textChapterStart[textCurChapter] || 0) + textCurColInChapter;
}

// 滚动模式下当前章节（不依赖 flipOn，模式切换前后都可用）
function scrollChapterIndex() {
  if (textBook && textChapterTop.length > 0) {
    // 虚拟化后 DOM 只有窗口内的占位 div，用高度前缀表二分定位
    const top = stripEl.scrollTop + 8;
    let lo = 0, hi = textChapterTop.length - 1;
    while (lo < hi) {
      const mid = (lo + hi + 1) >> 1;
      if ((textChapterTop[mid] || 0) <= top) lo = mid;
      else hi = mid - 1;
    }
    return Math.min(lo, Math.max(0, textChapterHeights.length - 1));
  }
  const els = stripEl.querySelectorAll('.page');
  if (els.length === 0) return 0;
  const top = stripEl.scrollTop + 8;
  let lo = 0, hi = els.length - 1, ans = 0;
  while (lo <= hi) {
    const mid = (lo + hi) >> 1;
    if (els[mid].offsetTop <= top) { ans = mid; lo = mid + 1; }
    else { hi = mid - 1; }
  }
  return ans;
}

// 当前滚动位置 → {章节, 章节内进度 0..1}
function textScrollPosition() {
  const ch = scrollChapterIndex();
  if (textBook && textChapterTop.length > 0) {
    const top = Math.max(0, stripEl.scrollTop + 8 - (textChapterTop[ch] || 0));
    const h = textChapterHeights[ch] || 120;
    return { chapter: ch, frac: h > 0 ? Math.max(0, Math.min(1, top / h)) : 0 };
  }
  const els = stripEl.querySelectorAll('.page');
  const holder = els[ch];
  if (!holder) return { chapter: 0, frac: 0 };
  const top = Math.max(0, stripEl.scrollTop - holder.offsetTop);
  const h = holder.offsetHeight;
  return { chapter: ch, frac: h > 0 ? Math.max(0, Math.min(1, top / h)) : 0 };
}

// 当前分页位置 → {章节, 章节内进度 0..1}
function textFlipPosition() {
  const pages = textChapterPages[textCurChapter] || 1;
  const frac = pages > 1 ? textCurColInChapter / (pages - 1) : 0;
  return { chapter: textCurChapter, frac: Math.max(0, Math.min(1, frac)) };
}

// ---- 文字书阅读百分比（滚动/翻页统一锚定到内容量）----

function textCharOffset(ch) {
  let acc = 0;
  for (let i = 0; i < ch && i < textChapterLengths.length; i++) acc += (textChapterLengths[i] || 1);
  return acc;
}

function textTotalCharCount() {
  return textTotalChars || 1;
}

// 百分比 → {章节, 章节内进度}
function progressToChapterWithin(progress) {
  const n = textChapterLengths.length || 1;
  const total = textTotalCharCount();
  progress = Math.max(0, Math.min(1, progress));
  const target = progress * total;
  let acc = 0;
  let ch = 0;
  for (let i = 0; i < n; i++) {
    const len = textChapterLengths[i] || 1;
    // 边界归属：进度恰好落在章末字符数时归下一章开头（而不是上一章末尾）。
    // 否则“第 N 章开头（如双页左页）”保存的百分比会因浮点/取整误差恢复成“第 N-1 章末尾”。
    // 1e-6 容差只吸收双精度误差（~1e-9 字符量级），不影响真实位置。
    if (acc + len > target + 1e-6 || i === n - 1) { ch = i; break; }
    acc += len;
  }
  const len = textChapterLengths[ch] || 1;
  const within = Math.max(0, Math.min(1, (target - acc) / len));
  return { chapter: ch, within };
}

// 当前阅读位置 → 全书百分比
function textProgress(flip) {
  const n = textChapterLengths.length || 1;
  let ch, within;
  if (flip) {
    ch = textCurChapter;
    const pages = textChapterPages[ch] || 1;
    within = pages > 1 ? textCurColInChapter / (pages - 1) : 0;
  } else {
    ch = scrollChapterIndex();
    within = 0;
    if (textChapterTop.length > 0) {
      // 虚拟化：用高度表计算章节内滚动进度
      const h = textChapterHeights[ch] || 120;
      const maxScroll = Math.max(1, h - stripEl.clientHeight);
      within = Math.max(0, Math.min(1, (stripEl.scrollTop + 8 - (textChapterTop[ch] || 0)) / maxScroll));
    }
  }
  within = Math.max(0, Math.min(1, within));
  const offset = textCharOffset(ch) + within * (textChapterLengths[ch] || 1);
  return offset / textTotalCharCount();
}

function textShowChapter(ch, colInChapter, animate) {
  textCurChapter = Math.max(0, ch);
  textCurColInChapter = Math.max(0, colInChapter);
  syncTextHolders(ch); // 窗口跟随 + 翻页模式下应用绝对定位
  const holder = holderFor(ch);
  if (holder) {
    const targetX = ch * textViewportW();
    if (animate && stripEl.scrollLeft !== targetX) {
      smoothScrollEl(stripEl, targetX, 300); // 跨章翻页：横向滑动过渡
    } else {
      stripEl.style.scrollBehavior = 'auto';
      stripEl.scrollLeft = targetX;
    }
  }
  const frame = textFrame(ch);
  if (frame) {
    try { frame.contentWindow.postMessage({ cshow: 'reader', type: 'goto', page: colInChapter }, '*'); } catch { /* 忽略 */ }
  }
  textCol = currentTextCol();
  updateFlipIndicator();
  updateTextNavSel();
  reconcileTextWindow(ch); // 前后章节缓存窗口随当前位置滚动
}

function textLocate(col, animate) {
  if (textTotalPages <= 0) return;
  col = Math.max(0, Math.min(col, textTotalPages - 1));
  const ch = textChapterOf(col);
  const colIn = col - (textChapterStart[ch] || 0);
  const pages = textChapterPages[ch] || 0;
  if (pages > 0 && textLoaded.has(ch)) {
    textPendingChapter = null;
    textShowChapter(ch, Math.min(colIn, pages - 1), animate);
    return;
  }
  // 目标章未加载/未测量：先用估算立即定位（不等待几何），就绪后校正
  const est = chapterPageCount(ch);
  textPendingChapter = ch;
  textPendingFrac = est > 1 ? Math.max(0, Math.min(1, colIn / (est - 1))) : 0;
  textPendingAnimate = false; // 几何校正瞬移
  textWaitChapter = null;
  textShowChapter(ch, Math.min(colIn, Math.max(0, est - 1)), animate);
  loadTextChapter(ch);
  reconcileTextWindow(ch);
}

function onTextGeom(ch, geom) {
  textChapterPages[ch] = (geom && geom.pages) || 1;
  updateCharsPerPageRef();
  rebuildTextOffsets();
  // 目录锚点列号已回报但几何未就绪：现在补定位到精确列
  if (pendingAnchorCol !== null && ch === textCurChapter) {
    const col = Math.min(pendingAnchorCol, (textChapterPages[ch] || 1) - 1);
    pendingAnchorCol = null;
    textShowChapter(ch, col);
  }
  // 当前章几何收敛后，若列号超出实际页数（估算边界漂移）则钳回
  if (ch === textCurChapter && textCurColInChapter >= (textChapterPages[ch] || 1)) {
    textCurColInChapter = Math.max(0, (textChapterPages[ch] || 1) - 1);
  }
  // 翻页模式：总页数随几何测量逐步精确，导航条同步重建成页级；
  // 只在变化较大时重建，避免大书每次窗口几何到达都重建几千个格子
  if (flipOn && textTotalPages !== textNavBuilt && Math.abs(textTotalPages - textNavBuilt) > 100) buildTextNav();
  // 通知等待该章几何的加载 promise
  const waiters = textGeomWaiters.get(ch);
  if (waiters) {
    textGeomWaiters.delete(ch);
    for (const w of waiters) { clearTimeout(w.timer); if (w.retry) clearTimeout(w.retry); w.resolve(); }
  }
  if (textWaitChapter === ch) {
    textWaitChapter = null;
    textLocate(textChapterStart[ch] || 0);
    return;
  }
  if (textPendingChapter === ch) {
    const frac = textPendingFrac;
    const animate = textPendingAnimate;
    textPendingChapter = null;
    const pages = textChapterPages[ch] || 1;
    const col = Math.round(frac * Math.max(0, pages - 1));
    textLocate((textChapterStart[ch] || 0) + col, animate);
    return;
  }
  textCol = currentTextCol();
  if (flipOn) { updateFlipIndicator(); updateTextNavSel(); }
}

function buildTextNav() {
  // 文字书不做页码/章节导航条：按页跳转用不到，目录已足够；
  // 也避免大书（如 8000+ 章）生成数万格子的性能问题。
  textNavBuilt = flipOn ? Math.max(1, textTotalPages || 1) : (epubMeta ? epubMeta.spine.length : 0);
}

function updateTextNavSel() {
  // 文字书无导航条：只更新目录高亮与阅读标题，无格子可遍历
  updateTocSel();
  updateReadingChapterTitle();
  updateWindowTitle();
  return;
}

async function buildTextFlip() {
  const en = entries[listSel];
  await ensureEpubStrip(en);
  if (!textBook) return 0;
  textChapterPages = new Array(epubMeta.spine.length).fill(0);
  textPendingChapter = null;
  textPendingFrac = 0;
  textPendingAnimate = false;
  textWaitChapter = null;
  updateCharsPerPageRef();
  rebuildTextOffsets();
  syncTextLoadedSet(); // 滚动模式已懒加载的章节并入窗口管理
  const n = epubMeta.spine.length;
  if (pendingVol && typeof pendingVol.progress === 'number') {
    // 优先用保存的内容百分比（滚动/翻页统一）
    const p = progressToChapterWithin(pendingVol.progress);
    textPendingChapter = Math.min(p.chapter, Math.max(0, n - 1));
    textPendingFrac = p.within;
  } else if (pendingVol && typeof pendingVol.page === 'number') {
    // 旧数据兜底（无 progress，只有 page/mode）
    if (pendingVol.mode === 'flip') {
      // 旧全局列 → 按估算偏移折算成章节 + 章节内进度
      const col = Math.max(0, pendingVol.page);
      const ch = Math.min(textChapterOf(col), n - 1);
      const est = chapterPageCount(ch);
      const colIn = col - (textChapterStart[ch] || 0);
      textPendingChapter = ch;
      textPendingFrac = est > 1 ? Math.max(0, Math.min(1, colIn / (est - 1))) : 0;
    } else {
      textPendingChapter = Math.min(pendingVol.page, Math.max(0, n - 1));
      textPendingFrac = 0;
    }
  } else {
    // 模式切换：从当前滚动位置捕获（章节 + 章节内进度），避免丢失阅读位置
    const pos = textScrollPosition();
    textPendingChapter = Math.min(pos.chapter, Math.max(0, n - 1));
    textPendingFrac = pos.frac;
  }
  pendingVol = null;
  // 先启动目标章 iframe 加载（与下方导航构建并行），几何通常会在构建期间就绪，
  // 避免入口被几何等待拖成 1~2 秒
  const entryCh = textPendingChapter;
  const entryFrac = textPendingFrac;
  if (entryCh !== null) loadTextChapter(entryCh);
  buildTextNav();
  broadcastReaderCfg();
  if (entryCh !== null) {
    const ch = entryCh;
    const frac = entryFrac;
    await loadTextChapter(ch); // 等目标章几何就绪，进入翻页即显示正确页
    finishPendingLocate(ch);
    if (textPendingChapter === ch) {
      // 超时兜底：几何迟迟未到也按估算定位，不卡住进入；
      // 保留 textPendingChapter，几何稍后到达时 onTextGeom 会自动校正到精确位置
      textShowChapter(ch, Math.round(frac * Math.max(0, chapterPageCount(ch) - 1)));
    }
  } else {
    textLocate(0);
  }
  return textCol;
}

// ---- 文字 EPUB：目录面板 ----

let tocAnchorCols = {};       // chapter -> {anchor: 列号}（reader 实测，目录高亮精确定位）
let tocAnchorsByChapter = {}; // chapter -> [anchor,...]（请求 reader 测量用）

function buildTocPanel() {
  tocListEl.innerHTML = '';
  const toc = epubMeta && epubMeta.toc;
  if (!toc || toc.length === 0) {
    const li = document.createElement('li');
    li.className = 'toc-empty';
    li.textContent = '本书无目录';
    tocListEl.appendChild(li);
    return;
  }
  tocAnchorsByChapter = {};
  const frag = document.createDocumentFragment();
  for (const item of toc) {
    const li = document.createElement('li');
    li.textContent = item.label;
    li.dataset.chapter = String(item.chapter);
    li.dataset.anchor = item.anchor || '';
    if (item.anchor) {
      (tocAnchorsByChapter[item.chapter] = tocAnchorsByChapter[item.chapter] || []).push(item.anchor);
    }
    li.addEventListener('click', () => {
      tocJumpTo(item);
      closeTocPanel();
    });
    frag.appendChild(li);
  }
  tocListEl.appendChild(frag);
}

// 请 reader 上报某章目录锚点的列号（用于目录高亮选中当前条目）
function requestTocAnchorCols(ch) {
  const anchors = tocAnchorsByChapter[ch];
  const frame = textFrame(ch);
  if (!anchors || anchors.length === 0 || !frame) return;
  try {
    frame.contentWindow.postMessage({ cshow: 'reader', type: 'anchorcols', anchors }, '*');
  } catch { /* 忽略 */ }
}

function tocJumpTo(item) {
  const n = epubMeta ? epubMeta.spine.length : 0;
  if (n === 0) return;
  const chapter = Math.max(0, Math.min(item.chapter, n - 1));
  const anchor = item.anchor || null;
  // 用「章节号 + 章节内进度」定位，避免 textChapterStart 估算偏移在
  // 一文件多章/估算偏差大的书上把目录跳转带偏（如跳到全书第一页）
  if (flipOn) textGotoChapterFrac(chapter, 0);
  else {
    (holderFor(chapter) || stripEl.querySelector('.page') || stripEl).scrollIntoView({ block: 'start' });
  }
  // 定位到文件内锚点（NCX 指向如 #filepos...），等目标章就绪后发给 reader
  if (anchor && textBook) {
    pendingAnchor = { chapter, anchor };
    sendPendingAnchor();
  } else {
    pendingAnchor = null; // 无锚点条目：清除残留的待定位锚点
  }
  updateTocSel();
}
// 把待定位锚点发给目标章 iframe（reader 就绪后处理；未就绪由 cshowReady 补发）
function sendPendingAnchor() {
  if (!pendingAnchor) return;
  const frame = textFrame(pendingAnchor.chapter);
  if (frame && frame.contentWindow) {
    try {
      frame.contentWindow.postMessage({ cshow: 'reader', type: 'anchor', anchor: pendingAnchor.anchor }, '*');
    } catch { /* 忽略 */ }
  }
}

let stripAnimToken = 0;

/// 父窗口横向缓动滚动（跨章翻页的滑动过渡；新滚动会打断旧动画）
function smoothScrollEl(el, x, duration) {
  const token = ++stripAnimToken;
  // CSS scroll-behavior:smooth 会拦截逐帧 scrollLeft 赋值（原生平滑与 rAF 打架），先强制 auto
  el.style.scrollBehavior = 'auto';
  const start = el.scrollLeft;
  const dist = x - start;
  if (Math.abs(dist) < 1) return;
  // 用 rAF 逐帧改 scrollLeft：对超长横向容器（8000+ 章、上千万像素）是廉价操作，
  // 不会像原生平滑/transform 那样触发大容器整体重排而阻塞主线程 2~3 秒。
  const t0 = performance.now();
  const ease = (p) => (p < 0.5 ? 4 * p * p * p : 1 - Math.pow(-2 * p + 2, 3) / 2);
  const step = () => {
    if (token !== stripAnimToken) return;
    const p = Math.min(1, (performance.now() - t0) / duration);
    el.scrollLeft = start + dist * ease(p);
    if (p < 1) requestAnimationFrame(step);
  };
  requestAnimationFrame(step);
}
function updateTocSel() {
  // 当前章节 + 章节内位置（翻页直接用 reader 汇报的列号，不走全局估算偏移）
  let cur, colIn = 0, pages = 1;
  if (flipOn) {
    cur = textCurChapter;
    colIn = textCurColInChapter;
    pages = Math.max(1, textChapterPages[cur] || 1);
  } else {
    cur = currentStripIndex();
    const pos = textScrollPosition();
    pages = Math.max(1, textChapterPages[cur] || 1);
    colIn = (pos && pos.chapter === cur) ? Math.max(0, Math.min(1, pos.frac)) * (pages - 1) : 0;
  }
  const cols = tocAnchorCols[cur] || {};
  const items = tocListEl.children;
  let first = -1, n = 0;
  const matches = [];
  for (let i = 0; i < items.length; i++) {
    items[i].classList.toggle('now', false);
    if (Number(items[i].dataset.chapter) === cur) {
      if (first === -1) first = i;
      const ac = items[i].dataset.anchor;
      matches.push({ i, col: ac ? cols[ac] : undefined });
      n++;
    }
  }
  if (first === -1) return;
  let pick = first;
  if (n > 1) {
    // 已知锚点列：取列号 ≤ 当前列的最大一条；未测量时按章节内进度在条目间估算
    let best = null;
    for (const m of matches) {
      if (typeof m.col === 'number' && m.col <= colIn + 0.5 && (best === null || m.col > best.col)) {
        best = m;
      }
    }
    if (best) {
      pick = best.i;
    } else {
      const frac = pages > 1 ? Math.max(0, Math.min(1, colIn / (pages - 1))) : 0;
      pick = first + Math.min(n - 1, Math.max(0, Math.round(frac * (n - 1))));
    }
  }
  const li = items[pick];
  if (li) li.classList.add('now');
}
function openTocPanel() {
  if (focus !== 'strip') return;
  if (textBook) {
    tocPanelTitleEl.textContent = '目录';
    buildTocPanel();
    tocPanelEl.classList.add('show');
    tocBackdropEl.hidden = false;
    tocBackdropEl.classList.add('show');
    updateTocSel();
    // 当前章若已加载，请 reader 实测锚点列号，让高亮精确到条目
    if (textLoaded.has(textCurChapter)) requestTocAnchorCols(textCurChapter);
  } else {
    tocPanelTitleEl.textContent = '页码';
    if (!tocListEl.querySelector('.page-toc-grid')) buildPageToc();
    tocPanelEl.classList.add('show');
    tocBackdropEl.hidden = false;
    tocBackdropEl.classList.add('show');
    updatePageTocSel();
  }
}
function closeTocPanel() {
  tocPanelEl.classList.remove('show');
  tocBackdropEl.classList.remove('show');
  tocBackdropEl.hidden = true;
}
tocBackdropEl.addEventListener('click', closeTocPanel);

function updateReadingChapterTitle() {
  if (!textBook || !epubMeta || focus !== 'strip') return;
  const ch = currentStripIndex();
  const ct = (epubMeta.chapter_titles && epubMeta.chapter_titles[ch]) || '';
  // 当前卷名随章节更新（跨册时窗口标题/标签同步切换）
  const vn = textVolumeOf(ch);
  if (vn) winTitleVol = vn;
  const base = vn || readingEl.dataset.base || readingEl.textContent || '';
  // 卷标记页的章节标题就是卷名本身（如 民调局异闻录2:清河鬼戏），避免重复显示
  const vnTail = vn ? vn.split(/[·•]/).pop().trim() : '';
  const showCt = ct && (!vnTail || !ct.includes(vnTail));
  readingEl.textContent = showCt ? (base + ' · ' + ct) : base;
  readingEl.title = readingEl.textContent;
  updateWindowTitle();
}

function changeFontSize(delta) {
  readerFontSize = Math.max(10, Math.min(32, readerFontSize + delta));
  persistReaderFont();
  onReaderFontChanged();
}
function cycleFontFamily() {
  const i = FONT_FAMILIES.indexOf(readerFontFamily);
  readerFontFamily = FONT_FAMILIES[(i + 1) % FONT_FAMILIES.length];
  persistReaderFont();
  updateFontButtons();
  onReaderFontChanged();
}
function onReaderFontChanged() {
  if (textBook && flipOn) {
    // 字号/字体影响全书分页：记录当前章节内进度（0..1），
    // 清空已测页数后按内容位置重新定位，避免跳回章节开头
    const cur = textCurChapter;
    const prevPages = textChapterPages[cur] || 1;
    const frac = prevPages > 1 ? Math.max(0, Math.min(1, textCurColInChapter / (prevPages - 1))) : 0;
    textChapterPages = textChapterPages.map(() => 0);
    updateCharsPerPageRef();
    rebuildTextOffsets();
    if (flipOn && textTotalPages !== textNavBuilt) buildTextNav();
    textPendingChapter = cur;
    textPendingFrac = frac;
    textPendingAnimate = false; // 字号变化定位不用滑动过渡
    textWaitChapter = null;
  } else if (textBook) {
    // 滚动模式：字号变化会重排章节高度，记录章节内进度，重排完成后恢复
    const pos = textScrollPosition();
    pendingScrollRestore = { chapter: pos.chapter, frac: pos.frac };
  }
  broadcastReaderCfg();
}

function setFocus(f) {
  if (focus === f) return;
  focus = f;
  if (f === 'strip') readingStartAt = Date.now();
  modeTabEl.hidden = f !== 'strip';
  if (f !== 'strip') pageModeEl.hidden = true;
  sidebarEl.style.display = f === 'strip' ? 'none' : 'flex'; // 阅读时隐藏侧栏（全宽阅读），退出恢复
  modeTabEl.classList.remove('show');
  pageModeEl.classList.remove('show');
  themeBtnEl.classList.remove('show');
  readerBackEl.classList.remove('show');
  versionEl.style.display = f === 'strip' ? 'none' : '';
  // 阅读：标题栏/底部状态栏默认隐藏，随控件呼出
  document.body.classList.toggle('reading', f === 'strip');
  document.body.classList.remove('ctl-on');
  if (f !== 'strip') {
    // 退出阅读：清空标题栏里的书名/卷信息，回到纯应用名
    winTitleBook = '';
    winTitleVol = '';
    updateWindowTitle();
  }
  updateReadingLabel();
  updateProgressBar();
}

// 窗口标题状态：书名（元数据优先）+ 卷名，供章节/页码更新时复用
let winTitleBook = '';
let winTitleVol = '';
let winTitleCache = '';
let textVolumes = []; // 每章所属卷名（目录卷标记检测，无卷则为 ''）

// 从目录识别“卷标记”：条目本身不像章节标题（第X章/序章/引子…），且紧跟一个章节标题条目。
// 典型如合集中的 “民调局异闻录2:清河鬼戏” → “第一章 河底洞坑”。
function computeTextVolumes() {
  textVolumes = [];
  const toc = epubMeta && epubMeta.toc;
  if (!toc || !epubMeta.spine) return;
  const isChapterLabel = (s) =>
    /^(第\s*[0-9一二三四五六七八九十百零]+章|序章|序言|引子|楔子|尾声|终章|番外|后记|前言|自序)/.test(s || '')
    || /^(Chapter|Ch\.?)\s+\S+/i.test(s || '')
    || /^(Prologue|Epilogue|Introduction|Preface|Foreword|Afterword)\b/i.test(s || '');
  const markers = [];
  for (let i = 0; i < toc.length; i++) {
    const cur = toc[i].label || '';
    // 显式卷标记（不强依赖后一条是章节）：中文卷/册/部/季/合集、数字+冒号
    // （民调局异闻录2:清河鬼戏）、“书名之卷名”（鬼吹灯之龙岭迷窟）、英文 Book/Part/Volume 前缀
    // （Book 1 by The Philosopher s Stone）
    const explicitVol = /(卷|册|部|季|合集)/.test(cur)
      || /[0-9一二三四五六七八九十百]+\s*[:：]/.test(cur)
      || /^.{2,10}之.+/.test(cur)
      || /^(Book|Part|Volume|Vol\.?|Section)\s+\S+/i.test(cur);
    // 只认显式卷标记；不再用“书名后紧跟 Chapter”猜卷，否则会把书与书之间的
    // 短篇/番外（如 Amber 的 Reflections in a Crystal Cave）误判成单独一卷。
    // 真实合集（Potter 的 Book 1 by…、Amber 的 Book One - …）都有 Book/Part/Volume 前缀。
    if (explicitVol) {
      if (isChapterLabel(cur)) continue; // 章节标题（如 Chapter One）不算卷
      let name = cur.trim();
      const zhi = cur.match(/^(.{2,10})之(.+)$/);
      if (zhi) {
        name = zhi[2].trim();
      } else {
        const byM = cur.match(/^(Book|Part|Volume|Vol\.?)\s+\S+\s+by\s+(.+)$/i);
        if (byM) {
          name = byM[2].trim();
        } else {
          // Book One - Nine Princes in Amber / Book One: The Guns of Avalon → 取分隔符后的书名
          const dashM = cur.match(/^(Book|Part|Volume|Vol\.?)\s+\S+\s*[-–—:：]\s*(.+)$/i);
          if (dashM) {
            name = dashM[2].trim();
          } else {
            const parts = cur.split(/[:：]/);
            if (parts.length > 1) {
              name = parts.pop().trim() || cur.trim();
              const num = cur.match(/([0-9]+)\s*[:：]/);
              if (num) name = '第' + num[1] + '册·' + name;
            }
          }
        }
      }
      markers.push({ chapter: toc[i].chapter, name });
    }
  }
  for (let ch = 0; ch < epubMeta.spine.length; ch++) {
    let name = '';
    for (const m of markers) {
      if (m.chapter <= ch) name = m.name;
    }
    textVolumes.push(name);
  }
}

function textVolumeOf(ch) {
  if (!epubMeta || !epubMeta.spine) return '';
  if (textVolumes.length !== epubMeta.spine.length) computeTextVolumes();
  return textVolumes[ch] || '';
}

function computeWindowTitle() {
  if (!winTitleBook && !winTitleVol) return 'cshow-gui';
  // 阅读模式：标题栏只显示书信息，不带应用名前缀
  let t = focus === 'strip' ? '《' + winTitleBook + '》' : 'cshow-gui - 《' + winTitleBook + '》';
  if (winTitleVol && winTitleVol !== winTitleBook) t += ' ' + winTitleVol;
  if (textBook && epubMeta) {
    // 文字书：书名后跟当前章节（含章节名）
    const ch = currentStripIndex();
    const ct = (epubMeta.chapter_titles && epubMeta.chapter_titles[ch]) || '';
    if (ct) t += ' · ' + ct;
  } else if (stripKind === 'pdf') {
    // PDF：没有章节概念，跟当前页码
    const idx = currentStripIndex();
    t += ' · 第 ' + (idx + 1) + ' 页';
  }
  return t;
}

function updateWindowTitle() {
  const t = computeWindowTitle();
  titlebarTextEl.textContent = t;
  if (t !== winTitleCache) {
    winTitleCache = t;
    setWindowTitle(t);
  }
}

// 阅读时底部显示“卷名”；窗口标题用书名（元数据优先）+ 章节/页码
function updateReadingLabel() {
  if (focus !== 'strip') {
    readingEl.style.display = 'none';
    return;
  }
  let book = '', vol = '';
  if (stripKind === 'images') {
    const folderName = cleanBookName(baseName(cwd)) || cwd;
    // 书名：元数据里录入的优先，未录入回退上级目录名
    const metaTitle = (bookMetaMap[cwd] && bookMetaMap[cwd].title) || '';
    const i = cwd.lastIndexOf('/');
    const dirBook = i > 0 ? cleanBookName(baseName(cwd.slice(0, i))) : '';
    book = metaTitle || dirBook;
    vol = folderName;
    // 阅读标签：优先元数据书名；文件夹名是真正的卷名才追加
    readingEl.dataset.base = book;
    readingEl.textContent = (vol && vol !== book) ? book + ' · ' + vol : book;
    readingEl.title = readingEl.textContent;
  } else {
    const filePath = stripKind === 'pdf' ? stripPdfPath : stripEpubPath;
    const base = filePath ? baseName(filePath).replace(/\.(pdf|epub)$/i, '') : '';
    const s = splitBookName(base);
    // 书名：元数据优先——先查文件本身，再查所在文件夹
    // （文件夹式多卷书把元数据设在文件夹上，如 漫画/机动战士高达 THE ORIGIN/卷1.epub）
    const parent = filePath ? filePath.slice(0, filePath.lastIndexOf('/')) : '';
    let metaTitle = (filePath && bookMetaMap[filePath] && bookMetaMap[filePath].title) || '';
    if (!metaTitle && parent && bookMetaMap[parent] && bookMetaMap[parent].title) {
      metaTitle = bookMetaMap[parent].title;
    }
    book = metaTitle || s.title;
    // 只有真正的卷/话/第N部分才作为卷名，避免把整个文件名重复拼在书名后面
    vol = (s.volume && s.volume !== s.title) ? s.volume : '';
    // 合集类书（如 民调局异闻录 正传 = 6 册）：用目录卷标记显示当前册
    if (textBook) {
      const vn = textVolumeOf(textCurChapter || 0);
      if (vn) vol = vn;
    }
    readingEl.dataset.base = vol || book;
    readingEl.textContent = vol || book;
    readingEl.title = readingEl.textContent;
  }
  winTitleBook = book;
  winTitleVol = vol;
  readingEl.style.display = 'block';
  updateWindowTitle();
}

// 方括号里是格式/描述标签（[Epub]/[PDF]/[全彩韩漫]…）时不算标题，一律不显示
const FORMAT_TAG = /^(epub|pdf|mobi|azw3|azw|fb2|txt|cbz|cbr|chm|docx?|漫画|韩漫|日漫|美漫|全彩|彩色|黑白|无修|汉化|简体|繁体|中文|扫图|电子书)$/i;
const DESCRIPTOR_TAG = /(韩漫|日漫|美漫|无修|汉化|扫图|电子书|合集|单行本|简体|繁体)/;
function isTagBracket(t) {
  return FORMAT_TAG.test(t) || DESCRIPTOR_TAG.test(t);
}

// 智能拆分书名：去扩展名/格式标签括号，返回 {title 系列名, volume 卷名}
function splitBookName(base) {
  if (!base) return { title: '', volume: '' };
  // 括号标题模式：末位括号 [标题] 后有内容（且括号内容不是格式标签）→ 该括号是标题
  const m = base.match(/\[([^\]]+)\](?=[^\[]*$)/);
  if (m) {
    const tag = m[1].trim();
    const after = base.slice(m.index + m[0].length).trim();
    if (after && !isTagBracket(tag)) {
      return { title: tag, volume: after };
    }
    // 这个括号不是标题（格式标签或无卷内容）：去掉该括号组后继续解析
    const cleaned = (base.slice(0, m.index) + ' ' + after).trim();
    if (cleaned !== base.trim()) return splitBookName(cleaned);
  }
  // 其余方括号（以及括号里的内容）一律不显示
  const noTags = base.replace(/\[[^\]]*\]/g, ' ').replace(/\s+/g, ' ').trim();
  // 中文卷/话标记：卷\d / 話\d / 话\d / 第\d
  const mark = noTags.search(/(卷|話|话|第)\s*\d/);
  if (mark > 0) {
    return {
      title: noTags.slice(0, mark).trim() || noTags.trim(),
      volume: noTags.slice(mark).trim() || noTags.trim(),
    };
  }
  return { title: noTags, volume: noTags };
}

// 书名显示名：去掉扩展名和 [Epub]/[PDF] 等格式标签括号
function cleanBookName(name) {
  const base = String(name || '').replace(/\.(pdf|epub)$/i, '');
  const s = splitBookName(base);
  return s.title || s.volume || base;
}

async function setWindowTitle(t) {
  try {
    await window.__TAURI__.window.getCurrentWindow().setTitle(t);
  } catch {
    document.title = t;
  }
}

function showPane(kind) {
  previewEl.style.display = kind === 'preview' ? 'flex' : 'none';
  stripEl.style.display = kind === 'strip' ? 'block' : 'none';
  progressEl.textContent = kind === 'strip' ? progressEl.textContent : '';
}

async function loadDir(path) {
  await savePosition(); // 离开前保存旧目录的位置
  cwd = path;
  currentIdx = -1;
  listSel = 0;
  stripBuilt = false;
  stripKind = 'images';
  stripPdfPath = null;
  stripEpubPath = null;
  epubMeta = null;
  pdfDoc = null;
  pendingPdfPage = 0;
  pendingEpubPage = 0;
  setFocus('list');
  entries = await invoke('list_dir', { path });
  images = entries.filter(e => e.is_image).map(e => e.path);
  // 记住当前目录，下次启动回到这里
  invoke('save_cwd', { path }).catch(() => {});
  stripEl.innerHTML = '';
  // 恢复上次浏览位置（current=文件名, page=页码）
  let savedName = '', savedPage = 0;
  try {
    const saved = await invoke('read_position', { dir: path });
    if (saved) {
      const o = JSON.parse(saved);
      savedName = o.current || '';
      savedPage = o.page || 0;
    }
  } catch { /* 无记录 */ }
  if (savedName) {
    const idx = entries.findIndex(en => en.name === savedName);
    if (idx >= 0) {
      listSel = idx;
      if (entries[idx].is_pdf) pendingPdfPage = savedPage;
      else if (entries[idx].is_epub || entries[idx].is_txt) pendingEpubPage = savedPage;
    }
  }
}

async function refreshFavorites() {
  try {
    favorites = await invoke('list_favorites');
  } catch {
    favorites = [];
  }
  renderLibTabs();
}

function activeLib() {
  return favorites.find(f => f.path === cwd) || favorites[0] || null;
}

// 书库顶栏：多个书库横向排列成 tab，点击切换
function renderLibTabs() {
  libTabsEl.innerHTML = '';
  const active = activeLib();
  if (!active) {
    libTabsEl.hidden = true;
    libTitleEl.hidden = false;
    libTitleEl.textContent = '请设置书库';
    libEyeEl.hidden = true;
    return;
  }
  libTabsEl.hidden = false;
  libTitleEl.hidden = true;
  libEyeEl.hidden = false;
  for (const fav of favorites) {
    const tab = document.createElement('div');
    tab.className = 'lib-tab' + (fav.path === active.path ? ' on' : '');
    const ic = libraryIconEl(fav.icon);
    ic.setAttribute('width', '12');
    ic.setAttribute('height', '12');
    const label = document.createElement('span');
    label.textContent = fav.alias || baseName(fav.path) || fav.path;
    tab.appendChild(ic);
    tab.appendChild(label);
    tab.title = fav.alias ? fav.alias + '（' + fav.path + '）' : fav.path;
    tab.addEventListener('click', () => {
      if (fav.path !== cwd) {
        loadDir(fav.path).then(async () => {
          if (libGridMode && !libGridBook) {
            libGridLibEntries = entries.slice();
            activeTags.clear();
            await renderLibBookPage();
          }
          renderLibTabs();
        });
      }
    });
    libTabsEl.appendChild(tab);
  }
  updateLibEye();
}

function updateLibEye() {
  const fav = activeLib();
  const hidden = fav ? fav.hidden : false;
  libEyeEl.textContent = '';
  libEyeEl.className = 'eye-toggle' + (hidden ? ' off' : '');
  libEyeEl.title = hidden ? '显示已隐藏的文件夹' : '隐藏 eye-off 的文件夹';
  libEyeEl.appendChild(eyeIconEl(hidden));
}

// 切换当前书库的 eye：隐藏时过滤被标记的书
libEyeEl.addEventListener('click', async () => {
  const fav = activeLib();
  if (!fav) return;
  let pwd;
  if (!fav.hidden) {
    // eye → eye-off：设置隐藏密码（可留空 = 不设密码，下次直接切换）
    pwd = await passwordDialog('设置隐藏密码（可留空，留空则下次直接切换）', '输入密码（可留空）');
    if (pwd === null) return;
  } else {
    if (!fav.has_password) {
      // 无密码：直接切换，不弹窗
      pwd = null;
    } else {
      // eye-off → eye：输入密码解锁
      pwd = await passwordDialog('输入密码解锁书库', '密码');
      if (pwd === null) return;
    }
  }
  let off = false;
  try {
    off = await invoke('toggle_eye', { path: fav.path, password: pwd });
  } catch (e) {
    if (typeof e === 'string' && e.includes('密码')) showAlert(e);
    else if (e && e.message && String(e.message).includes('密码')) showAlert(String(e.message));
    return;
  }
  fav.hidden = off;
  fav.has_password = off ? (pwd !== null && pwd !== '') : false;
  updateLibEye();
  if (cwd === fav.path) {
    entries = await invoke('list_dir', { path: cwd }).catch(() => entries);
    if (libGridMode && !libGridBook) {
      libGridLibEntries = entries.slice();
      renderLibBookPage();
    }
  }
});

// ---- 书库管理（齿轮）----

const libGearEl = document.getElementById('lib-gear');
const libDialogEl = document.getElementById('lib-dialog');
const libDialogCloseEl = document.getElementById('lib-dialog-close');
const libListEl = document.getElementById('lib-list');
const libUpEl = document.getElementById('lib-up');
const libPathEl = document.getElementById('lib-path');
const libDirsEl = document.getElementById('lib-dirs');
const libAddEl = document.getElementById('lib-add');
const libStateEl = document.getElementById('lib-state');
const libApiKeyEl = document.getElementById('lib-apikey-input');
const libApiKeySaveEl = document.getElementById('lib-apikey-save');
const libApiKeyStateEl = document.getElementById('lib-apikey-state');
let libBrowsePath = '';

libGearEl.appendChild(gearIconEl());

// ---- AI 设置（DeepSeek API Key，存书库管理对话框）----

async function loadApiKeyInput() {
  libApiKeyEl.value = '';
  libApiKeyStateEl.textContent = '';
  try {
    const key = await invoke('get_deepseek_key');
    libApiKeyEl.value = key || '';
  } catch { /* 读不到就留空 */ }
}

async function saveApiKey() {
  const key = libApiKeyEl.value.trim();
  libApiKeyStateEl.textContent = '保存中…';
  try {
    await invoke('set_deepseek_key', { key });
    libApiKeyStateEl.textContent = '已保存';
    setTimeout(() => { libApiKeyStateEl.textContent = ''; }, 1500);
  } catch (e) {
    libApiKeyStateEl.textContent = '保存失败：' + (typeof e === 'string' ? e : JSON.stringify(e));
  }
}

libApiKeySaveEl.addEventListener('click', saveApiKey);
libApiKeyEl.addEventListener('keydown', (e) => {
  if (e.key === 'Enter') { e.stopPropagation(); saveApiKey(); }
});

function openLibDialog() {
  libBrowsePath = cwd || (favorites[0] && favorites[0].path) || '/';
  loadApiKeyInput();
  renderLibList();
  renderLibDirs();
  libDialogEl.hidden = false;
}

function closeLibDialog() {
  libDialogEl.hidden = true;
}

function renderLibList() {
  libListEl.innerHTML = '';
  for (let i = 0; i < favorites.length; i++) {
    const fav = favorites[i];
    const li = document.createElement('li');

    // 图标：点击循环切换
    const iconBtn = document.createElement('button');
    iconBtn.className = 'lib-icon-btn';
    iconBtn.title = '切换图标';
    iconBtn.appendChild(libraryIconEl(fav.icon));
    iconBtn.addEventListener('click', async (ev) => {
      ev.stopPropagation();
      const keys = ['', ...LIBRARY_ICONS];
      const cur = keys.indexOf(fav.icon || '');
      const next = keys[(cur + 1) % keys.length];
      const aliasInput = li.querySelector('.lib-alias-input');
      const alias = aliasInput ? aliasInput.value.trim() : (fav.alias || '');
      await invoke('set_library_meta', { path: fav.path, alias, icon: next }).catch(() => {});
      await refreshFavorites();
      renderLibList();
    });

    // 别名
    const aliasInput = document.createElement('input');
    aliasInput.className = 'lib-alias-input';
    aliasInput.value = fav.alias || '';
    aliasInput.placeholder = baseName(fav.path) || fav.path;
    aliasInput.title = fav.path;
    aliasInput.addEventListener('click', (ev) => ev.stopPropagation());
    aliasInput.addEventListener('change', async () => {
      await invoke('set_library_meta', { path: fav.path, alias: aliasInput.value.trim(), icon: fav.icon || '' }).catch(() => {});
      await refreshFavorites();
    });
    aliasInput.addEventListener('keydown', (e) => {
      e.stopPropagation();
      if (e.key === 'Enter') aliasInput.blur();
      else if (e.key === 'Escape') closeLibDialog();
    });

    // 上移 / 下移
    const up = document.createElement('button');
    up.className = 'lib-move';
    up.textContent = '↑';
    up.title = '上移';
    up.disabled = i === 0;
    up.addEventListener('click', async (ev) => {
      ev.stopPropagation();
      const order = favorites.map(f => f.path);
      const j = i - 1;
      if (j < 0) return;
      [order[i], order[j]] = [order[j], order[i]];
      await invoke('reorder_libraries', { paths: order }).catch(() => {});
      await refreshFavorites();
      renderLibList();
    });

    const down = document.createElement('button');
    down.className = 'lib-move';
    down.textContent = '↓';
    down.title = '下移';
    down.disabled = i === favorites.length - 1;
    down.addEventListener('click', async (ev) => {
      ev.stopPropagation();
      const order = favorites.map(f => f.path);
      const j = i + 1;
      if (j >= order.length) return;
      [order[i], order[j]] = [order[j], order[i]];
      await invoke('reorder_libraries', { paths: order }).catch(() => {});
      await refreshFavorites();
      renderLibList();
    });

    // 移出书库
    const rm = document.createElement('button');
    rm.className = 'rm';
    rm.textContent = '×';
    rm.title = '移出书库';
    rm.addEventListener('click', async (ev) => {
      ev.stopPropagation();
      await invoke('toggle_favorite', { path: fav.path }).catch(() => {});
      await refreshFavorites();
      renderLibList();
      renderLibDirs();
      if (cwd === fav.path && favorites.length) {
        await loadDir(favorites[0].path); // 当前书库被移除，切到剩余的第一个
      }
      if (libGridMode && !libGridBook) {
        libGridLibEntries = entries.slice();
        renderLibBookPage();
      }
    });

    li.appendChild(iconBtn);
    li.appendChild(aliasInput);
    li.appendChild(up);
    li.appendChild(down);
    li.appendChild(rm);
    li.addEventListener('click', () => {
      libBrowsePath = fav.path;
      renderLibDirs();
    });
    libListEl.appendChild(li);
  }
}

async function renderLibDirs() {
  libPathEl.textContent = libBrowsePath;
  libPathEl.title = libBrowsePath;
  libDirsEl.innerHTML = '';
  libAddEl.textContent = '添加此文件夹';
  let entries = [];
  try {
    entries = await invoke('list_dir', { path: libBrowsePath });
  } catch { /* 打不开的目录显示为空 */ }
  for (const en of entries) {
    if (!en.is_dir) continue;
    const li = document.createElement('li');
    li.textContent = en.name + '/';
    li.addEventListener('click', () => {
      libBrowsePath = en.path;
      renderLibDirs();
    });
    libDirsEl.appendChild(li);
  }
  const inLib = favorites.some(f => f.path === libBrowsePath);
  libStateEl.textContent = inLib ? '已在书库' : '';
  libAddEl.style.display = inLib ? 'none' : '';
}

libGearEl.addEventListener('click', openLibDialog);
libDialogCloseEl.addEventListener('click', closeLibDialog);
libDialogEl.addEventListener('click', (e) => {
  if (e.target === libDialogEl) closeLibDialog();
});

// 阅读统计按钮
libStatsEl.appendChild(barChartIconEl());
libStatsEl.addEventListener('click', openStatsDialog);

// 元数据编辑对话框
metaDialogCloseEl.addEventListener('click', closeMetaDialog);
metaCancelEl.addEventListener('click', closeMetaDialog);
metaSaveEl.addEventListener('click', saveMetaDialog);
metaSmartEl.addEventListener('click', smartFetchMeta);
metaFixTocEl.addEventListener('click', fixTocClick);
metaCoverPickEl.addEventListener('click', () => metaCoverFileEl.click());
metaCoverFileEl.addEventListener('change', () => {
  const f = metaCoverFileEl.files && metaCoverFileEl.files[0];
  if (!f) return;
  if (f.size > 20 * 1024 * 1024) {
    metaCoverStatusEl.textContent = '图片过大（>20MB）';
    return;
  }
  const reader = new FileReader();
  reader.onload = () => {
    const dataUrl = String(reader.result || '');
    const comma = dataUrl.indexOf(',');
    metaPendingCover = { name: f.name, data: comma >= 0 ? dataUrl.slice(comma + 1) : '' };
    metaCoverPreviewEl.src = dataUrl;
    metaCoverPreviewEl.hidden = false;
    metaCoverRemoveEl.hidden = false;
    metaCoverStatusEl.textContent = '';
  };
  reader.onerror = () => { metaCoverStatusEl.textContent = '读取图片失败'; };
  reader.readAsDataURL(f);
  metaCoverFileEl.value = '';
});
metaCoverRemoveEl.addEventListener('click', () => {
  metaPendingCover = 'remove';
  metaCoverPreviewEl.hidden = true;
  metaCoverPreviewEl.removeAttribute('src');
  metaCoverRemoveEl.hidden = true;
  metaCoverStatusEl.textContent = '';
});
metaDialogEl.addEventListener('click', (e) => {
  if (e.target === metaDialogEl) closeMetaDialog();
});
metaDialogEl.addEventListener('keydown', (e) => {
  if (e.key === 'Escape') { e.stopPropagation(); closeMetaDialog(); }
  else if (e.key === 'Enter' && e.target && e.target.tagName === 'INPUT') { e.stopPropagation(); saveMetaDialog(); }
});

// 阅读统计对话框
statsDialogCloseEl.addEventListener('click', closeStatsDialog);
statsDialogEl.addEventListener('click', (e) => {
  if (e.target === statsDialogEl) closeStatsDialog();
});
statsDialogEl.addEventListener('keydown', (e) => {
  if (e.key === 'Escape') { e.stopPropagation(); closeStatsDialog(); }
});

// ---- 漫画卷尾：下一卷对话框 ----
let nextVolCooldownUntil = 0; // 取消后的冷却，避免滚动抖动立刻再弹
let stripUserScrolled = false; // 本次阅读会话是否发生过真实滚动（防进入时误触发卷尾弹窗）

function nextComicVolume() {
  if (!stripReturn || !libGridVols || libGridVols.length < 2) return null;
  if (textBook) return null; // 文字书是章节分页，不走卷尾
  return libGridVols[stripReturn.volIndex + 1] || null;
}

function showNextVolumeDialog() {
  const next = nextComicVolume();
  if (!next || !libGridBook) return false;
  // 左：下一卷封面预览
  nextVolCoverEl.innerHTML = '';
  const img = document.createElement('img');
  img.alt = '';
  if (next.thumb) img.src = convertFileSrc(next.thumb);
  nextVolCoverEl.appendChild(img);
  // 右上：本书累计阅读时间（含本次会话）
  const base = (stripReturn && readTimes[stripReturn.ebookPath]) || 0;
  const elapsed = readingStartAt ? Math.round((Date.now() - readingStartAt) / 1000) : 0;
  nextVolTimeEl.textContent = '本书累计阅读 ' + fmtDuration(base + elapsed);
  // 中：下一本 / 卷名
  const s = splitBookName(next.name.replace(/\.(pdf|epub)$/i, ''));
  nextVolNameEl.textContent = s.volume || s.title || next.name;
  nextVolDialogEl.hidden = false;
  nextVolContinueEl.focus();
  return true;
}

function hideNextVolumeDialog() {
  nextVolDialogEl.hidden = true;
}

async function continueNextVolume() {
  if (nextVolDialogEl.hidden) return;
  const next = nextComicVolume();
  hideNextVolumeDialog();
  if (!next || !libGridBook) return;
  // 刷新本次阅读时长到本书（openVolume 的 loadDir 会顺带保存当前卷位置）
  if (readingStartAt) {
    const secs = Math.round((Date.now() - readingStartAt) / 1000);
    const key = stripReturn ? stripReturn.ebookPath : cwd;
    if (secs > 0 && key) {
      readTimes[key] = (readTimes[key] || 0) + secs;
      invoke('add_reading_time', { path: key, seconds: secs })
        .then(total => { readTimes[key] = total; })
        .catch(() => {});
    }
  }
  // 标记当前卷读完并保存位置（连读切换不走 exitStripMode，这里必须显式保存）
  await saveVolumePositionForExit({});
  openVolume(libGridBook, next, true); // 连读：下一卷从第一页开始，忽略已存位置
}

function cancelNextVolume() {
  if (nextVolDialogEl.hidden) return;
  hideNextVolumeDialog();
  nextVolCooldownUntil = Date.now() + 2000; // 2 秒内不因滚动抖动再弹
}

function exitNextVolume() {
  if (nextVolDialogEl.hidden) return;
  hideNextVolumeDialog();
  exitStripMode();
}

nextVolContinueEl.addEventListener('click', continueNextVolume);
nextVolCancelEl.addEventListener('click', cancelNextVolume);
nextVolExitEl.addEventListener('click', exitNextVolume);
nextVolDialogEl.addEventListener('click', (e) => {
  if (e.target === nextVolDialogEl) cancelNextVolume();
});
nextVolDialogEl.addEventListener('keydown', (e) => {
  if (e.key === 'Enter') { e.stopPropagation(); continueNextVolume(); }
  else if (e.key === 'Escape') { e.stopPropagation(); cancelNextVolume(); }
});
libUpEl.addEventListener('click', () => {
  const i = libBrowsePath.lastIndexOf('/');
  if (i > 0) {
    libBrowsePath = libBrowsePath.slice(0, i);
    renderLibDirs();
  }
});
libAddEl.addEventListener('click', async () => {
  await invoke('toggle_favorite', { path: libBrowsePath }).catch(() => {});
  await refreshFavorites();
  renderLibList();
  renderLibDirs();
  if (favorites.some(f => f.path === libBrowsePath)) {
    await loadDir(libBrowsePath); // 新书库设为当前
  }
  if (libGridMode && !libGridBook) {
    libGridLibEntries = entries.slice();
    renderLibBookPage();
  }
});

async function generateVolumeThumb(v, holder) {
  let saved = null;
  try {
    if (v.kind === 'pdf') {
      saved = await renderPdfThumb(v.path);
    } else if (v.kind === 'epub' || v.kind === 'txt') {
      const cover = v.kind === 'epub' ? await invoke('epub_cover', { path: v.path }) : null;
      if (cover) {
        saved = cover;
      } else {
        const title = v.name.replace(/\.(epub|txt)$/i, '');
        saved = await makeTextThumb(title, v.path);
      }
    }
  } catch { /* 生成失败，走占位 */ }
  if (!saved) {
    if (holder.label) holder.label.textContent = v.name + '（无缩略图）';
    return;
  }
  holder.img.src = convertFileSrc(saved);
}

// 用 pdf.js 渲染 PDF 第一页，存成缓存缩略图
async function renderPdfThumb(pdfPath) {
  const doc = await pdfjsLib.getDocument({ url: convertFileSrc(pdfPath) }).promise;
  const page = await doc.getPage(1);
  const base = page.getViewport({ scale: 1 });
  const scale = 360 / base.width;
  const vp = page.getViewport({ scale });
  const canvas = document.createElement('canvas');
  canvas.width = Math.floor(vp.width);
  canvas.height = Math.floor(vp.height);
  await page.render({ canvasContext: canvas.getContext('2d'), viewport: vp }).promise;
  return savePngThumb(canvas, pdfPath);
}

// EPUB 没有封面时，画一张标题占位图作为缩略图
async function makeTextThumb(title, keyPath) {
  const canvas = document.createElement('canvas');
  canvas.width = 320;
  canvas.height = 440;
  const ctx = canvas.getContext('2d');
  const dark = window.matchMedia && window.matchMedia('(prefers-color-scheme: dark)').matches;
  const bg0 = dark ? '#1d2530' : '#ffffff';
  const bg1 = dark ? '#14171c' : '#f3f4f6';
  const stroke = dark ? '#2a2f36' : '#d5dae1';
  const gold = dark ? '#e8c65a' : '#d97706';
  const titleColor = dark ? '#dfe3e8' : '#1f2937';
  const labelColor = dark ? '#8a93a0' : '#9aa1ab';
  const g = ctx.createLinearGradient(0, 0, 0, 440);
  g.addColorStop(0, bg0);
  g.addColorStop(1, bg1);
  ctx.fillStyle = g;
  ctx.fillRect(0, 0, canvas.width, canvas.height);
  ctx.strokeStyle = stroke;
  ctx.lineWidth = 2;
  ctx.strokeRect(10, 10, 300, 420);
  ctx.fillStyle = gold;
  ctx.fillRect(40, 64, 70, 4);
  ctx.fillStyle = titleColor;
  ctx.font = '600 28px system-ui, "PingFang SC", sans-serif';
  ctx.textBaseline = 'top';
  const words = String(title || 'EPUB').split('');
  let line = '', y = 90;
  for (const ch of words) {
    if (ctx.measureText(line + ch).width > canvas.width - 80) {
      ctx.fillText(line, 40, y);
      y += 36;
      line = ch;
    } else {
      line += ch;
    }
    if (y > 330) break;
  }
  if (line) ctx.fillText(line, 40, y);
  ctx.fillStyle = labelColor;
  ctx.font = '500 16px system-ui, sans-serif';
  ctx.fillText('EPUB', 40, 376);
  return savePngThumb(canvas, keyPath);
}

async function savePngThumb(canvas, keyPath) {
  const blob = await new Promise((res) => canvas.toBlob(res, 'image/png'));
  const buf = await blob.arrayBuffer();
  return invoke('save_thumb', { path: keyPath, data: Array.from(new Uint8Array(buf)) });
}

function openVolume(en, v, fresh) {
  const volIndex = libGridVols.indexOf(v);
  // 连读切换下一卷时，cwd 是当前卷目录，不能当书库根；保留原有的 parentDir
  const parentDir = fresh && stripReturn ? stripReturn.parentDir : cwd;
  stripReturn = {
    parentDir,
    ebookPath: en.path,
    volIndex: Math.max(0, volIndex),
  };
  const target = v.kind === 'imgdir' ? v.path : en.path;
  loadDir(target).then(() => {
    if (v.kind === 'imgdir') {
      pendingVol = fresh ? null : {
        page: typeof v.saved_page === 'number' ? v.saved_page : null,
        mode: v.saved_mode || null,

        progress: typeof v.saved_progress === 'number' ? v.saved_progress : null,
      };
      enterStripMode(fresh ? 0 : v.saved_page);
      return;
    }
    const idx = entries.findIndex(e => e.path === v.path);
    if (idx < 0) return;
    listSel = idx;
    if (v.kind === 'pdf') {
      pendingVol = fresh ? null : {
        page: typeof v.saved_page === 'number' ? v.saved_page : null,
        mode: v.saved_mode || null,

        progress: typeof v.saved_progress === 'number' ? v.saved_progress : null,
      };
      enterPdfStrip(fresh ? 0 : v.saved_page);
    } else {
      pendingVol = fresh ? null : {
        page: typeof v.saved_page === 'number' ? v.saved_page : null,
        mode: v.saved_mode || null,

        progress: typeof v.saved_progress === 'number' ? v.saved_progress : null,
      };
      enterEpubStrip(undefined, fresh);
    }
  });
}

// 条漫模式下点击左侧的 PDF/EPUB 分卷：切换到该分卷并恢复其阅读位置
async function switchStripTo(en) {
  listSel = entries.indexOf(en);
  let savedPage;
  let savedMode;
  const root = await invoke('ebook_root', { dir: cwd }).catch(() => null);
  if (root) {
    try {
      const vols = await invoke('ebook_volumes', { dir: root });
      const v = vols.find(x => x.path === en.path);
      if (v && typeof v.saved_page === 'number') savedPage = v.saved_page;
      if (v && v.saved_mode) savedMode = v.saved_mode;
    } catch { /* 无记录 */ }
  }
  if (stripReturn) {
    const idx = entries.findIndex(x => x.path === en.path);
    stripReturn.volIndex = Math.max(0, idx);
  }
  if (en.is_pdf) {
    pendingVol = {
      page: typeof savedPage === 'number' ? savedPage : null,
      mode: savedMode || null,
    };
    await enterPdfStrip(typeof savedPage === 'number' ? savedPage : 0);
  } else if (en.is_epub || en.is_txt) {
    pendingVol = {
      page: typeof savedPage === 'number' ? savedPage : null,
      mode: savedMode || null,
    };
    await enterEpubStrip();
  }
  updateReadingLabel();
}

// 退出条漫时，把当前分卷的阅读位置写进所属电子书文件夹
async function saveVolumePositionForExit(opts = {}) {
  if (currentIdx < 0 || pages.length === 0) return;
  // 在 await 之前同步捕获会被 exitStripMode 后续清理掉的状态（否则保存会拿到 null）
  const kind = stripKind === 'images' ? 'imgdir' : stripKind;
  const epubPath = stripEpubPath;
  const cwdAtSave = cwd;
  const wasFlip = opts.flip ?? flipOn;
  const wasDouble = opts.double ?? doublePage;
  const wasTextBook = opts.textBook ?? textBook;
  // 文字书：保存内容百分比（滚动/翻页统一，不受窗口尺寸/字号影响）
  if (kind === 'epub' && wasTextBook) {
    const progress = (typeof opts.progress === 'number') ? opts.progress : 0;
    const finished = progress >= 0.995;
    const root = await invoke('ebook_root', { dir: cwdAtSave }).catch(() => null);
    if (!root) return;
    await invoke('save_volume_position', {
      ebookDir: root,
      volumePath: epubPath,
      kind,
      page: 0,
      total: 0,
      mode: wasFlip ? 'flip' : 'scroll',
      finished,
      progress,
    }).catch(() => {});
    return;
  }
  const idx = currentIdx;
  const pageList = pages;
  const item = pageList[idx];
  // EPUB 用 epub 文件本身作为卷路径（翻页模式下 pages 是图片页，不能用图片路径）
  const volumePath = kind === 'imgdir' ? cwdAtSave : (kind === 'epub' ? epubPath : item.path);
  const page = wasFlip && wasDouble ? Math.floor(idx / 2) * 2 : idx;
  // 读完判定：单页=最后一页；双页=覆盖最后一张图的跨页
  const lastIdx = pageList.length - 1;
  let finished;
  if (wasFlip && wasDouble) {
    const lastSpread = Math.floor(lastIdx / 2) * 2;
    finished = Math.floor(idx / 2) * 2 >= lastSpread;
  } else {
    finished = idx >= lastIdx;
  }
  const root = await invoke('ebook_root', { dir: cwdAtSave }).catch(() => null);
  if (!root) return;
  if (kind === 'imgdir' && cwdAtSave === root) return; // 电子书根目录本身不算图片分卷
  await invoke('save_volume_position', {
    ebookDir: root,
    volumePath,
    kind,
    page,
    total: pageList.length,
    mode: wasFlip ? 'flip' : 'scroll',
    finished,
  }).catch(() => {});
}

/// 记录阅读中当前页索引（列表高亮已随侧栏列表移除）
function highlightIndex(idx) {
  if (idx < 0 || idx >= pages.length) return;
  currentIdx = idx;
}

// ---- 图片条漫 ----

async function ensureStrip() {
  if (stripBuilt && stripKind === 'images') return;
  stripBuilt = true;
  stripKind = 'images';
  stripPdfPath = null;
  stripEl.innerHTML = '';
  if (images.length === 0) {
    stripEl.innerHTML = '<div class="hint">这个目录没有图片</div>';
    return;
  }
  const dims = await invoke('image_dims', { paths: images });
  pages = images.map(p => ({ path: p, name: baseName(p), kind: 'img' }));
  for (let i = 0; i < images.length; i++) {
    const img = document.createElement('img');
    img.className = 'page';
    img.decoding = 'async';
    img.dataset.src = images[i];
    img.alt = '';
    const d = dims && dims[i];
    if (d) {
      img.style.aspectRatio = `${d[0]} / ${d[1]}`;
      img.dataset.ratio = `${d[0]} / ${d[1]}`;
    }
    stripEl.appendChild(img);
  }
  const io = new IntersectionObserver((ents) => {
    for (const en of ents) {
      if (!en.isIntersecting) continue;
      const el = en.target;
      if (!el.src) el.src = convertFileSrc(el.dataset.src);
      io.unobserve(el);
    }
  }, { root: stripEl, rootMargin: '600px 0px' });
  for (const img of stripEl.querySelectorAll('img')) io.observe(img);
}

async function enterStripMode(startPage) {
  await ensureStrip();
  if (images.length === 0) {
    stripReturn = null; // 没有图片可进条漫，丢弃返回上下文
    showPane('preview');
    return;
  }
  stripUserScrolled = false; // 新会话：重置滚动标记，防止进入时误触发卷尾弹窗
  showPane('strip');
  setFocus('strip');
  let target;
  if (typeof startPage === 'number' && startPage >= 0) {
    target = Math.min(startPage, images.length - 1);
  } else {
    const sel = entries[listSel];
    const idx = sel && sel.is_image ? images.indexOf(sel.path) : (currentIdx >= 0 ? currentIdx : 0);
    target = idx >= 0 ? idx : 0;
  }
  stripEl.querySelectorAll('.page')[target].scrollIntoView({ block: 'start' });
  highlightIndex(target);
  updateReadingLabel();
  buildPageToc();
  loadFlipSettings();
}

// ---- PDF 条漫 ----

async function ensurePdfStrip(en) {
  if (stripBuilt && stripKind === 'pdf' && stripPdfPath === en.path) return;
  stripBuilt = true;
  stripKind = 'pdf';
  stripPdfPath = en.path;
  stripEl.innerHTML = '';
  try {
    pdfDoc = await pdfjsLib.getDocument({ url: convertFileSrc(en.path) }).promise;
  } catch (err) {
    pdfDoc = null;
    console.error('PDF load failed:', en.path, err);
    return;
  }
  pages = [];
  const holders = [];
  const viewports = await Promise.all(
    Array.from({ length: pdfDoc.numPages }, (_, i) =>
      pdfDoc.getPage(i + 1).then(p => p.getViewport({ scale: 1 }))
    )
  );
  for (let i = 0; i < pdfDoc.numPages; i++) {
    const vp = viewports[i];
    const holder = document.createElement('div');
    holder.className = 'page pdf';
    holder.style.aspectRatio = `${vp.width} / ${vp.height}`;
    holder.dataset.page = String(i + 1);
    stripEl.appendChild(holder);
    holders.push(holder);
    pages.push({ path: en.path, name: en.name, kind: 'pdf' });
  }
  const io = new IntersectionObserver((ents) => {
    for (const en2 of ents) {
      if (!en2.isIntersecting) continue;
      const holder = en2.target;
      if (holder.dataset.rendered) { io.unobserve(holder); continue; }
      holder.dataset.rendered = '1';
      io.unobserve(holder);
      renderPdfPage(pdfDoc, parseInt(holder.dataset.page, 10), holder);
    }
  }, { root: stripEl, rootMargin: '600px 0px' });
  for (const h of holders) io.observe(h);
}

async function renderPdfPage(pdf, pageNum, holder) {
  try {
    const page = await pdf.getPage(pageNum);
    const vp = page.getViewport({ scale: 1 });
    // 按阅读区宽度 + 设备像素比渲染，翻页/双页下也清晰
    const targetW = Math.max(holder.clientWidth || 0, stripEl.clientWidth || 800);
    let scale = (targetW * (window.devicePixelRatio || 1)) / vp.width;
    if (vp.width * scale > 4096) scale = 4096 / vp.width; // 防止超大画布
    const rvp = page.getViewport({ scale });
    const canvas = document.createElement('canvas');
    canvas.width = Math.floor(rvp.width);
    canvas.height = Math.floor(rvp.height);
    canvas.style.width = '100%';
    canvas.style.height = '100%';
    await page.render({ canvasContext: canvas.getContext('2d'), viewport: rvp }).promise;
    holder.appendChild(canvas);
  } catch {
    holder.textContent = '渲染失败';
  }
}

async function enterPdfStrip(savedPage) {
  const en = entries[listSel];
  if (!en || !en.is_pdf) return;
  await ensurePdfStrip(en);
  if (!pdfDoc) {
    showPane('preview');
    previewEl.innerHTML = '<div class="hint">PDF 打开失败</div>';
    return;
  }
  stripUserScrolled = false;
  showPane('strip');
  setFocus('strip');
  const page = typeof savedPage === 'number' ? savedPage : pendingPdfPage;
  const target = Math.min(page, pages.length - 1);
  const els = stripEl.querySelectorAll('.page');
  (els[target] || els[0]).scrollIntoView({ block: 'start' });
  highlightIndex(target);
  updateReadingLabel();
  buildPageToc();
  loadFlipSettings();
}


// ---- EPUB 条漫 ----

async function ensureEpubStrip(en) {
  if (stripBuilt && stripKind === 'epub' && stripEpubPath === en.path) return;
  stripBuilt = true;
  stripKind = 'epub';
  stripEpubPath = en.path;
  epubBookToken = Date.now().toString(36) + Math.random().toString(36).slice(2, 8);
  stripEl.innerHTML = '';
  try {
    epubMeta = await invoke('open_epub', { path: en.path });
  } catch {
    epubMeta = null;
    return;
  }
  textBook = !!epubMeta.text_book;
  textChapterPages = [];
  textChapterStart = [];
  textTotalPages = 0;
  textCol = 0;
  textCurChapter = 0;
  textCurColInChapter = 0;
  textPendingChapter = null;
  textWaitChapter = null;
  textPendingFrac = 0;
  textPendingAnimate = false;
  textLoaded = new Set();
  textLoadPromises = new Map();
  textGeomWaiters = new Map();
  textCharsPerPage = 0;
  pendingAnchor = null;
  pendingScrollRestore = null;
  textChapterLengths = (epubMeta.chapter_lengths && epubMeta.chapter_lengths.length === epubMeta.spine.length)
    ? epubMeta.chapter_lengths
    : new Array(epubMeta.spine.length).fill(1);
  textTotalChars = textChapterLengths.reduce((a, b) => a + (b || 1), 0);
  modeTabEl.classList.toggle('text-book', textBook);
  if (textIo) textIo.disconnect();
  textHolderLo = 0;
  textHolderHi = -1;
  textSpacerBefore = null;
  textSpacerAfter = null;
  pages = [];
  for (let i = 0; i < epubMeta.spine.length; i++) {
    pages.push({ path: en.path, name: en.name, kind: 'epub' });
  }
  const makeIo = () => {
    const io = new IntersectionObserver((ents) => {
    for (const en2 of ents) {
      if (!en2.isIntersecting) continue;
      const holder = en2.target;
      // strip 未布局（隐藏/尚未显示）时所有元素会被判为相交；
      // 跳过，避免一次性创建并加载全部章节 iframe（几百个 iframe 会淹没目标章几何）
      if (!stripEl.clientWidth) continue;
      const ch = parseInt(holder.dataset.chapter, 10) || 0;
      const frame = textFrame(ch);
      if (frame && !frame.getAttribute('src')) frame.src = frame.dataset.src;
      textLoaded.add(ch);
      io.unobserve(holder);
    }
    }, { root: stripEl, rootMargin: '600px 0px' });
    return io;
  };
  if (textBook) {
    // 文字书：占位 div 虚拟化——只保留当前章窗口（syncTextHolders 按需补齐）
    initTextHeightTable();
    textIo = makeIo();
  } else {
    // 图像书：保持全量占位 + IO（每章一个 iframe 显示整页图）
    for (let i = 0; i < epubMeta.spine.length; i++) {
      const holder = document.createElement('div');
      holder.className = 'page epub';
      holder.dataset.chapter = String(i);
      stripEl.appendChild(holder);
    }
    textIo = makeIo();
    for (const h of stripEl.querySelectorAll('.page.epub')) textIo.observe(h);
  }
}

async function enterEpubStrip(savedPage, fresh) {
  const en = entries[listSel];
  if (!en || (!en.is_epub && !en.is_txt)) return;
  // 立即盖住进入过程（列表 → 解包 → 阅读），避免侧栏/界面切换闪现；
  // 解包进度条 z-index 更高，仍会显示在遮罩上方
  showTextLoading('正在打开…');
  if (!pendingVol && !fresh) {
    // 从列表直接进入：也查分卷记录，保证翻页位置可恢复
    try {
      const vols = await invoke('ebook_volumes', { dir: cwd });
      const v = vols.find(x => x.path === en.path);
      if (v) {
        pendingVol = {
          page: typeof v.saved_page === 'number' ? v.saved_page : null,
          mode: v.saved_mode || null,

          progress: typeof v.saved_progress === 'number' ? v.saved_progress : null,
        };
      }
    } catch { /* 无记录 */ }
  }
  stripEntryPending = true; // 进入阅读：解包阶段显示顶部进度
  await ensureEpubStrip(en);
  stripEntryPending = false;
  if (!epubMeta) {
    hideTextLoading();
    showPane('preview');
    previewEl.innerHTML = '<div class="hint">EPUB 打开失败</div>';
    return;
  }
  if (!textBook) hideTextLoading(); // 图像书无需章节加载遮罩
  stripUserScrolled = false;
  showPane('strip');
  setFocus('strip');
  // 虚拟化：先建目标章窗口的占位 div，scrollIntoView 才能定位
  const targetHolder = holderFor(0);
  // 文字书：用保存的内容百分比恢复滚动位置（翻页位置由 buildTextFlip 恢复）
  if (typeof savedPage !== 'number' && textBook && pendingVol && typeof pendingVol.progress === 'number') {
    const p = progressToChapterWithin(pendingVol.progress);
    const target = Math.min(p.chapter, Math.max(0, pages.length - 1));
    (holderFor(target) || targetHolder).scrollIntoView({ block: 'start' });
    pendingScrollRestore = p.within > 0 ? { chapter: target, frac: p.within } : null;
  } else {
    let page;
    if (fresh) {
      // 连读进入下一卷：忽略书目录的浏览位置（pendingEpubPage 可能还是上一卷的页码），从第一页开始
      page = 0;
    } else {
      page = typeof savedPage === 'number' ? savedPage : pendingEpubPage;
      // 滚动模式：优先用已保存的章节（跨会话也有效）；翻页模式的位置由 buildTextFlip 精确恢复
      if (typeof savedPage !== 'number' && pendingVol && typeof pendingVol.page === 'number' && pendingVol.mode !== 'flip') {
        page = pendingVol.page;
      }
    }
    const target = Math.min(page, pages.length - 1);
    (holderFor(target) || targetHolder).scrollIntoView({ block: 'start' });
  }
  highlightIndex(currentStripIndex());
  updateReadingLabel();
  buildPageToc();
  await loadFlipSettings();
  if (textBook) {
    await waitTextEntryReady();
    hideTextLoading();
  }
}

// 章节 iframe 上报高度/几何/就绪，用于稳定布局与文字分页
window.addEventListener('message', (e) => {
  const d = e.data;
  if (!d || typeof d !== 'object') return;
  let frame = null;
  for (const f of stripEl.querySelectorAll('iframe')) {
    if (f.contentWindow === e.source) { frame = f; break; }
  }
  if (!frame) return;
  if (typeof d.cshowWheel === 'number') {
    // 文字书 iframe 的触控板滚轮转交父窗口统一翻整页
    if (flipOn && focus === 'strip') {
      handleFlipWheel(d.cshowWheel);
    }
    return;
  }
  if (typeof d.cshowH === 'number') {
    // 滚动模式按内容高度；文字书翻页时高度由视口接管
    if (!(textBook && flipOn)) {
      if (textBook) {
        // 虚拟化：记录实测章节高度，更新前缀表与 spacer（撑起窗口外空间）
        const ch = parseInt(frame.dataset.chapter, 10) || 0;
        if (ch < textChapterHeights.length && textChapterHeights[ch] !== d.cshowH) {
          textChapterHeights[ch] = d.cshowH;
          rebuildTextChapterTop();
          updateTextSpacers();
        }
      }
      frame.style.height = d.cshowH + 'px';
      // 翻页→滚动切换后：目标章高度就绪时恢复章节内滚动进度
      if (pendingScrollRestore && textBook) {
        const ch = parseInt(frame.dataset.chapter, 10) || 0;
        if (ch === pendingScrollRestore.chapter) {
          const holder = frame.closest('.page');
          if (holder) {
            const maxScroll = Math.max(0, holder.offsetHeight - stripEl.clientHeight);
            // 虚拟化：章节顶部偏移用前缀表，不用 holder.offsetTop（holder 可能不在窗口内）
            stripEl.scrollTop = (textChapterTop[ch] || 0) + pendingScrollRestore.frac * maxScroll;
          }
          pendingScrollRestore = null;
        }
      }
    }
    return;
  }
  if (d.cshowReady) {
    if (textBook) sendReaderCfgTo(frame); // 应用当前主题/字号/模式
    requestTocAnchorCols(parseInt(frame.dataset.chapter, 10) || 0);
    // 目标章就绪后补发目录跳转锚点（此前消息可能已丢失）
    if (pendingAnchor && textBook && (parseInt(frame.dataset.chapter, 10) || 0) === pendingAnchor.chapter) {
      try {
        frame.contentWindow.postMessage({ cshow: 'reader', type: 'anchor', anchor: pendingAnchor.anchor }, '*');
      } catch { /* 忽略 */ }
    }
    return;
  }
  if (d.cshowGeom) {
    const ch = parseInt(frame.dataset.chapter, 10) || 0;
    if (textBook) onTextGeom(ch, d.cshowGeom);
    requestTocAnchorCols(ch);
    return;
  }
  if (d.cshowAnchorCols) {
    // reader 上报的目录锚点列号：缓存并刷新目录高亮
    const ch = parseInt(frame.dataset.chapter, 10) || 0;
    tocAnchorCols[ch] = (d.cshowAnchorCols && d.cshowAnchorCols.cols) || {};
    updateTocSel();
    return;
  }
  if (d.cshowAnchor) {
    // 翻页模式：reader 上报锚点在章节内的精确列号，直接用章节号+列号定位，
    // 不再经 textChapterStart 全局估算偏移（一文件多章、估算偏差大的书会定位错乱）
    if (flipOn && textBook) {
      const ch = parseInt(frame.dataset.chapter, 10) || 0;
      const col = Math.max(0, d.cshowAnchor.col || 0);
      const pages = textChapterPages[ch] || 0;
      // 清掉待定位状态，避免几何就绪后 finishPendingLocate 用章首覆盖锚点列
      textPendingChapter = null;
      textPendingFrac = 0;
      if (pages > 0) {
        textShowChapter(ch, Math.min(col, pages - 1));
      } else {
        // 几何尚未上报：先到章首，onTextGeom 就绪后补定位到精确列
        pendingAnchorCol = col;
        textShowChapter(ch, 0);
      }
    }
    pendingAnchor = null;
    return;
  }
  if (d.cshowAnchorDone) {
    pendingAnchor = null;
    return;
  }
});
// ---- 通用 ----

function exitStripMode() {
  const wasFlip = flipOn;
  const wasDouble = doublePage;
  // 文字书状态要在保存位置前捕获，否则下方清理把 textBook/textCol 清零会导致保存错位置
  const wasTextBook = textBook;
  const wasTextCol = textCol;
  const wasTextTotalPages = textTotalPages;
  // 文字书：在重置前算好阅读百分比（flipOn/章节状态此刻仍有效）
  const wasTextProgress = textBook ? textProgress(wasFlip) : null;
  // 累计本次阅读时长到当前书（持久化到 .cshow，同时更新内存缓存）
  if (readingStartAt) {
    const elapsed = Math.round((Date.now() - readingStartAt) / 1000);
    const bookKey = stripReturn ? stripReturn.ebookPath : (stripEpubPath || stripPdfPath || cwd);
    if (elapsed > 0 && bookKey) {
      readTimes[bookKey] = (readTimes[bookKey] || 0) + elapsed;
      invoke('add_reading_time', { path: bookKey, seconds: elapsed })
        .then(total => { readTimes[bookKey] = total; })
        .catch(() => {});
    }
    readingStartAt = 0;
  }
  hideUnpackBar();
  hideTextLoading();
  stripEntryPending = false;
  if (!wasFlip) savePosition(); // 翻页模式的位置由分卷记录保存，避免污染章节位置
  flipOn = false;
  stripEl.classList.remove('flip', 'double', 'rtl');
  flipEpub = null;
  pendingVol = null;
  flipWheelAccum = 0;
  flipCooldown = 0;
  doublePage = false;
  flipSettingsPromise = null;
  flipVolumeKey = null;
  // 文字书状态清理
  textBook = false;
  textChapterPages = [];
  textChapterStart = [];
  textTotalPages = 0;
  textCol = 0;
  textPendingChapter = null;
  textWaitChapter = null;
  textPendingFrac = 0;
  textPendingAnimate = false;
  for (const ch of Array.from(textLoaded)) unloadTextChapter(ch); // 释放已加载章节
  textLoaded = new Set();
  textLoadPromises = new Map();
  textGeomWaiters = new Map();
  textCharsPerPage = 0;
  pendingAnchor = null;
  pendingScrollRestore = null;
  // 虚拟化状态清理（占位 div/spacer 随 stripEl.innerHTML 清空一并移除）
  textChapterHeights = [];
  textChapterTop = [];
  textHolderLo = 0;
  textHolderHi = -1;
  textSpacerBefore = null;
  textSpacerAfter = null;
  if (textIo) { textIo.disconnect(); textIo = null; }
  if (textHolderScrollRaf) { cancelAnimationFrame(textHolderScrollRaf); textHolderScrollRaf = 0; }
  modeTabEl.classList.remove('text-book');
  closeTocPanel();
  stripEl.style.overflowX = '';
  stripEl.style.overflowY = '';
  winTitleCache = '';
  setWindowTitle('cshow-gui');
  if (stripKind === 'pdf' && currentIdx >= 0) pendingPdfPage = currentIdx;
  if (stripKind === 'epub' && currentIdx >= 0) pendingEpubPage = currentIdx;
  setFocus('list');
  if (stripReturn) {
    // 从书架进来的分卷：退出后回到主文件夹，焦点落在刚退出的分卷上
    const ret = stripReturn;
    stripReturn = null;
    // 先记住这个分卷的位置并等它落盘，再刷新书架，否则书签可能读不到
    saveVolumePositionForExit({ flip: wasFlip, double: wasDouble, textBook: wasTextBook, textCol: wasTextCol, textTotalPages: wasTextTotalPages, progress: wasTextProgress }).then(() => {
      if (libGridReadingReturn) {
        // 从图标卷页进入的阅读：退出后回到卷页
        const g = libGridReadingReturn;
        libGridReadingReturn = null;
        showPane('preview'); // 隐藏条漫残留内容，右侧回到预览
        if (g.direct && g.libPath) {
          // 单卷书直接进入：退出回到书库页并选中这本书
          loadDir(g.libPath).then(() => {
            openLibGrid().then(() => {
              const idx = libGridCards.findIndex(c => c.path === g.book.path);
              if (idx >= 0) {
                libGridSel = idx;
                updateLibSel();
              }
            });
          });
          return;
        }
        // 回到卷页前先恢复书库根目录（阅读期间 cwd 已切到书目录），
        // 否则后续 Esc 回书库页时 refreshBookPage 会读到书目录的旧条目
        loadDir(ret.parentDir).then(() => {
          openLibGrid(g.book).then(() => {
            libGridSel = Math.min(g.volIndex, Math.max(0, libGridVols.length - 1));
            updateLibSel();
          });
        });
        return;
      }
      loadDir(ret.parentDir).then(() => {
        openLibGrid().then(() => {
          const en = entries.find(e => e.path === ret.ebookPath);
          if (en) {
            const idx = libGridCards.findIndex(c => c.path === en.path);
            if (idx >= 0) {
              libGridSel = idx;
              updateLibSel();
            }
          }
        });
      });
    });
    // 重置条漫构建状态：下次进入时重新解包判定文字/图像书，避免复用旧状态
    stripBuilt = false;
    stripKind = 'images';
    stripPdfPath = null;
    stripEpubPath = null;
    epubMeta = null;
    pdfDoc = null;
    pages = [];
    currentIdx = -1;
    return;
  }
  // 非书架路径（从列表直接进的分卷）：异步记位置，不阻塞退出
  const savePromise = saveVolumePositionForExit({ flip: wasFlip, double: wasDouble, textBook: wasTextBook, textCol: wasTextCol, textTotalPages: wasTextTotalPages, progress: wasTextProgress });
  // 重置条漫构建状态（saveVolumePositionForExit 已同步读取 stripKind）
  stripBuilt = false;
  stripKind = 'images';
  stripPdfPath = null;
  stripEpubPath = null;
  epubMeta = null;
  pdfDoc = null;
  if (currentIdx >= 0) {
    const path = pages[currentIdx] && pages[currentIdx].path;
    const idx = path ? entries.findIndex(en => en.path === path) : -1;
    if (idx >= 0) listSel = idx;
  }
  pages = [];
  currentIdx = -1;
  if (libGridBookReturn) {
    libGridBookReturn = false;
    const retEntry = entries[listSel] || null; // 刚退出的散装书
    // 等位置落盘后再刷新书库页，保证「最近阅读」排序读到最新 last_read_at
    savePromise.then(() => {
      openLibGrid().then(() => {
        if (retEntry) {
          const idx = libGridCards.findIndex(c => c.path === retEntry.path);
          if (idx >= 0) {
            libGridSel = idx;
            updateLibSel();
          }
        }
      });
    });
  }
}

// 视口顶部对应的页下标（二分查找，滚动时不会逐个读布局）
function currentStripIndex() {
  if (stripKind === 'epub' && textBook && !flipOn && textChapterTop.length > 0) {
    // 虚拟化：滚动模式用高度前缀表定位（DOM 只有窗口内占位 div）
    return scrollChapterIndex();
  }
  const els = stripEl.querySelectorAll('.page');
  if (els.length === 0) return 0;
  let lo = 0, hi = els.length - 1, ans = 0;
  if (flipOn) {
    // 文字书翻页：scrollLeft / 视口宽 即章节下标（双页在 iframe 内部，不乘 2）
    if (stripKind === 'epub' && textBook) {
      return Math.round(Math.abs(stripEl.scrollLeft) / textViewportW());
    }
    // 翻页模式为横向布局：scrollLeft / 跨页宽 即页码。
    // WKWebView 里 RTL 双页（grid）的 scrollLeft 为负值（0 → -(scrollWidth-clientWidth)），
    // 而 RTL 单页（flex）是正值，取绝对值把两种语义统一成「距首页的横向距离」。
    const pageW = Math.max(1, stripEl.clientWidth);
    const k = Math.round(Math.abs(stripEl.scrollLeft) / pageW);
    return doublePage ? k * 2 : k;
  }
  const top = stripEl.scrollTop + 8;
  while (lo <= hi) {
    const mid = (lo + hi) >> 1;
    if (els[mid].offsetTop <= top) { ans = mid; lo = mid + 1; }
    else { hi = mid - 1; }
  }
  return ans;
}

// 把当前位置（当前页文件名 + 页码）写进目录的 .cshow
async function savePosition() {
  if (!cwd || pages.length === 0 || currentIdx < 0 || currentIdx >= pages.length) return;
  const item = pages[currentIdx];
  try {
    await invoke('save_position', { dir: cwd, name: item.name, page: currentIdx });
  } catch { /* 目录只读等场景静默跳过 */ }
}

function scrollStrip(delta) {
  // 平滑滚动：滚动（条漫）模式的翻页不再是瞬间跳变
  stripEl.scrollBy({ top: delta, behavior: 'smooth' });
}

function scrollStripPage(dir) {
  scrollStrip(dir * Math.max(stripEl.clientHeight * 0.9, 100));
}

function scrollToPage(idx, animate) {
  if (stripKind === 'epub' && textBook && flipOn) {
    textLocate(idx);
    return;
  }
  const els = stripEl.querySelectorAll('.page');
  if (els.length === 0) return;
  idx = Math.max(0, Math.min(idx, els.length - 1));
  flipAnchor = idx;
  const targetIdx = (flipOn && doublePage) ? Math.floor(idx / 2) * 2 : idx;
  const targetEl = els[targetIdx];
  if (flipOn && animate && !rtl) {
    // 与文字书一致：300ms 缓动横向滑动过渡（RTL 双页滚动语义特殊，保留瞬移）
    smoothScrollEl(stripEl, targetEl.offsetLeft, 300);
  } else {
    targetEl.scrollIntoView({ block: 'start', inline: 'start' });
  }
  highlightIndex(idx);
  updatePageTocSel();
  updateFlipIndicator();
}

// ---- 翻页模式（条漫/翻页 tab）----

function currentBookDir() {
  if (stripKind === 'images') {
    const i = cwd.lastIndexOf('/');
    const parent = i > 0 ? cwd.slice(0, i) : cwd;
    // 单层图片书（书目录直接在书库根目录下）：设置按书目录本身，避免整库串用
    if (favorites.some(f => f.path === parent)) return cwd;
    return parent;
  }
  return cwd;
}

// 散装书（文件直接在书库根目录下）：阅读设置按文件隔离，避免整库串用
function currentVolumeKey() {
  if (stripKind === 'images') return null;
  const filePath = stripKind === 'pdf' ? stripPdfPath : stripEpubPath;
  if (!filePath) return null;
  const parent = filePath.slice(0, filePath.lastIndexOf('/'));
  if (parent === cwd && favorites.some(f => f.path === cwd)) return filePath;
  return null;
}

function loadFlipSettings() {
  flipSettingsPromise = (async () => {
    const bookDir = currentBookDir();
    flipBookDir = bookDir;
    flipVolumeKey = currentVolumeKey();
    try {
      const s = await invoke('read_book_settings', { ebookDir: bookDir, volume: flipVolumeKey });
      // 文字书未设置时默认翻页模式；图片/PDF 书默认滚动
      flipOn = s.read_mode == null ? !!textBook : s.read_mode === 'flip';
      rtl = !!s.rtl;
      // 文字书未设置时默认双页
      doublePage = s.double_page == null ? !!textBook : !!s.double_page;
      // 单书字体/字号：读本书设置，未设置（null）则沿用全局默认
      if (textBook) {
        if (typeof s.font_size === 'number' && s.font_size >= 10 && s.font_size <= 32) readerFontSize = s.font_size;
        if (FONT_FAMILIES.includes(s.font_family)) readerFontFamily = s.font_family;
        updateFontButtons();
      }
      // 单书阅读背景：本书设置了就用本书的；未设置时文字书默认羊皮纸，
      // 其余回退全局默认（避免沿用上一本书的主题，否则看起来像“全换了”）
      if (READER_THEMES.includes(s.theme)) {
        applyReaderTheme(s.theme);
      } else if (textBook) {
        applyReaderTheme('sepia');
      } else {
        applyReaderTheme(await loadReaderTheme(), false);
      }
    } catch {
      flipOn = false;
      rtl = false;
      doublePage = false;
    }
    if (flipOn) {
      let target;
      if (stripKind === 'epub') {
        target = textBook ? await buildTextFlip() : await buildEpubFlipPages();
      } else {
        // PDF/纯图片的页本身就是一页，直接复用；恢复保存的翻页位置
        target = (pendingVol && typeof pendingVol.page === 'number') ? pendingVol.page : currentStripIndex();
      }
      applyFlipMode();
      scrollToPage(target);
    } else {
      applyFlipMode();
    }
    pendingVol = null;
  })();
  return flipSettingsPromise;
}

function applyFlipMode() {
  stripEl.classList.toggle('flip', flipOn);
  stripEl.classList.toggle('double', flipOn && doublePage);
  // 文字书分页暂不支持 RTL（列序由 iframe 内部决定），避免破坏章节横向定位
  stripEl.classList.toggle('rtl', flipOn && rtl && !(stripKind === 'epub' && textBook));
  // showPane 设置了内联 display:block，翻页需要覆盖成横向布局
  if (flipOn) {
    if (stripKind === 'epub' && textBook) {
      // 文字书（虚拟化）：窗口 holder 绝对定位，strip 自身撑出全书宽度 n×视口宽；
      // 双页在 iframe 内部用 CSS 多列实现。不用 flex+宽度 spacer（部分 WKWebView 渲染失效）
      stripEl.style.display = 'block';
      stripEl.style.position = 'relative';
      stripEl.style.overflowX = 'hidden';
      stripEl.style.overflowY = 'hidden';
      syncTextHolders(textCurChapter); // 翻页激活：立即应用绝对定位布局
    } else {
      stripEl.style.display = doublePage ? 'grid' : 'flex';
      stripEl.style.overflowX = '';
      stripEl.style.overflowY = '';
    }
  } else {
    stripEl.style.display = 'block';
    stripEl.style.position = '';
    stripEl.style.width = '';
    stripEl.style.overflowX = '';
    stripEl.style.overflowY = '';
  }
  modeScrollBtn.classList.toggle('on', !flipOn);
  modeFlipBtn.classList.toggle('on', flipOn);
  modeTabEl.hidden = !(focus === 'strip');
  pageModeEl.hidden = !(focus === 'strip' && flipOn);
  pageSingleBtn.classList.toggle('on', !doublePage);
  pageDoubleBtn.classList.toggle('on', doublePage);
  const pageH = Math.max(100, stripEl.clientHeight) + 'px';
  for (const page of stripEl.querySelectorAll('.page')) {
    if (page.tagName === 'IMG') {
      if (flipOn) {
        page.style.height = pageH;
        page.style.width = '100%';
        page.style.objectFit = 'contain';
        page.style.aspectRatio = 'auto';
      } else {
        page.style.height = '';
        page.style.width = '';
        page.style.objectFit = '';
        page.style.aspectRatio = page.dataset.ratio || '';
      }
    } else if (page.classList.contains('pdf')) {
      page.style.height = flipOn ? pageH : '';
    } else if (page.classList.contains('epub')) {
      const f = page.querySelector('iframe');
      if (flipOn && textBook) {
        page.style.width = '100%';
        page.style.height = '100%';
        page.style.flexShrink = '0';
        if (f) { f.style.width = '100%'; f.style.height = '100%'; }
      } else if (!flipOn) {
        page.style.width = '';
        page.style.height = '';
        page.style.flexShrink = '';
        if (f) { f.style.width = ''; f.style.height = ''; }
      }
    }
  }
  updateFlipIndicator();
  if (textBook) broadcastReaderCfg(); // 模式/几何变化同步到各章 iframe
}

async function setReadMode(mode) {
  if (flipSettingsPromise) await flipSettingsPromise;
  const next = mode === 'flip';
  if (next === flipOn) return;
  // 文字书切换模式有加载/重排过程（翻页等几何、滚动重建章节），用遮罩盖住
  const isText = !!(stripKind === 'epub' && textBook);
  if (isText) showTextLoading(next ? '正在切换翻页模式…' : '正在切换滚动模式…');
  try {
    let target;
    if (next) {
      if (stripKind === 'epub') {
        if (textBook) flipOn = true; // 先置位：buildTextFlip 依此下发分页配置
        target = textBook ? await buildTextFlip() : await buildEpubFlipPages();
      } else {
        target = currentStripIndex();
      }
      flipOn = true;
      applyFlipMode();
      scrollToPage(target); // 布局改变后重新定位
    } else {
      target = currentStripIndex();
      // 文字书：切换前记录内容百分比，切回滚动后据此恢复
      const textPos = (stripKind === 'epub' && textBook) ? { progress: textProgress(true) } : null;
      flipOn = false;
      applyFlipMode();
      if (stripKind === 'epub') {
        await restoreEpubStrip(textPos !== null ? textPos : target);
      } else {
        scrollToPage(target);
      }
    }
  } finally {
    if (isText) {
      await waitTextEntryReady(); // 翻页已等过几何；滚动等目标章高度就绪
      hideTextLoading();
    }
  }
  saveFlipSettings();
}

async function setPageMode(mode) {
  if (stripKind === 'epub' && textBook) {
    // 单/双页切换会重排当前章，用遮罩盖住重新测量/定位过程
    const isFlip = flipOn;
    if (isFlip) showTextLoading(mode === 'double' ? '正在切换双页模式…' : '正在切换单页模式…');
    // 单双页切换会重排分栏：记录当前章节内进度（0..1），
    // 切换后按新列数重映射回同一内容位置，避免固定跳回章节开头
    const cur = textCurChapter;
    const prevPages = textChapterPages[cur] || 1;
    const frac = prevPages > 1 ? Math.max(0, Math.min(1, textCurColInChapter / (prevPages - 1))) : 0;
    doublePage = mode === 'double';
    // 重置每章列数：只重测窗口内已加载章节，其余按字符量估算，就绪后按 frac 精确定位
    textChapterPages = new Array(textChapterPages.length).fill(0);
    updateCharsPerPageRef();
    rebuildTextOffsets();
    applyFlipMode();
    if (flipOn) buildTextNav();
    saveFlipSettings();
    textGotoChapterFrac(cur, frac);
    if (isFlip) {
      await waitPageModeReady(cur);
      hideTextLoading();
    }
    return;
  }
  const target = doublePage ? Math.floor(flipAnchor / 2) * 2 : flipAnchor;
  doublePage = mode === 'double';
  applyFlipMode();
  scrollToPage(target);
  saveFlipSettings();
}

function saveFlipSettings() {
  if (flipBookDir) {
    invoke('write_book_settings', {
      ebookDir: flipBookDir,
      volume: flipVolumeKey,
      readMode: flipOn ? 'flip' : 'scroll',
      rtl,
      doublePage,
      fontSize: readerFontSize,
      fontFamily: readerFontFamily,
    }).catch(() => {});
  }
}

async function toggleRtl() {
  rtl = !rtl;
  applyFlipMode();
  saveFlipSettings();
}

// 底部阅读进度条：按当前页 / 总页数填充
function updateProgressBar() {
  if (focus !== 'strip') {
    readProgressEl.classList.remove('on');
    return;
  }
  if (stripKind === 'epub' && textBook && flipOn) {
    const total = textTotalPages || 1;
    if (total <= 1) { readProgressEl.classList.remove('on'); return; }
    readProgressEl.classList.add('on');
    readProgressFillEl.style.width = Math.min(100, Math.round(((textCol + 1) / total) * 100)) + '%';
    return;
  }
  if (pages.length === 0) {
    readProgressEl.classList.remove('on');
    return;
  }
  readProgressEl.classList.add('on');
  const idx = currentStripIndex();
  const pct = pages.length > 1 ? Math.min(100, Math.round(((idx + 1) / pages.length) * 100)) : 100;
  readProgressFillEl.style.width = pct + '%';
}

function updateFlipIndicator() {
  if (focus === 'strip' && flipOn) {
    if (stripKind === 'epub' && textBook) {
      const total = textTotalPages || 1;
      if (doublePage) {
        const end = Math.min(textCol + 1, total - 1);
        progressEl.textContent = (textCol + 1) + '–' + (end + 1) + ' / ' + total + ' 页';
      } else {
        progressEl.textContent = (textCol + 1) + ' / ' + total + ' 页';
      }
    } else {
      const cur = flipCurrent();
      if (flipOn && doublePage) {
        const end = Math.min(cur + 1, pages.length - 1);
        progressEl.textContent = (cur + 1) + '–' + (end + 1) + ' / ' + pages.length + ' 页';
      } else {
        progressEl.textContent = (cur + 1) + ' / ' + pages.length + ' 页';
      }
    }
  }
  updateProgressBar();
}

function flipCurrent() {
  if (stripKind === 'epub' && textBook && flipOn) return textCol;
  const cur = currentStripIndex();
  return doublePage ? Math.floor(cur / 2) * 2 : cur;
}

function flipTo(dir) {
  if (stripKind === 'epub' && textBook && flipOn) {
    const step = doublePage ? 2 : 1;
    const n = textChapterPages.length;
    if (n === 0) return;
    let ch = textCurChapter;
    let colIn = textCurColInChapter + dir * step;
    // 跨章边界：一次翻页最多跨一章（向后停在下章开头、向前停在上章末尾/开头）。
    // 不能用估算页数连续跨章——未加载章节按字符量估算可能只有 1 页，
    // 双页翻页的跨章余量 ≥ 估算页数时会一次跳过整章（如第 7 章 → 第 9 章）。
    if (colIn >= chapterPageCount(ch) && ch < n - 1) {
      colIn -= chapterPageCount(ch);
      ch++;
      colIn = 0; // 停在下章开头，保证章节标题可见
    }
    if (colIn < 0 && ch > 0) {
      ch--;
      colIn += chapterPageCount(ch);
      if (colIn < 0) colIn = 0; // 上章比余量还短时停在其开头（保守，保证可见）
    }
    // 钳制到全书首尾
    if (ch === 0 && colIn < 0) colIn = 0;
    if (ch === n - 1 && colIn >= chapterPageCount(ch)) colIn = Math.max(0, chapterPageCount(ch) - 1);
    if (ch === textCurChapter) {
      // 同章内翻页：当前章已加载并测量，直接显示
      textShowChapter(ch, Math.min(colIn, Math.max(0, (textChapterPages[ch] || 1) - 1)));
    } else {
      // 跨章：目标章若未加载则按需加载，几何就绪后按章节内进度定位
      const est = chapterPageCount(ch);
      textGotoChapterFrac(ch, est > 1 ? Math.max(0, Math.min(1, colIn / (est - 1))) : 0, true); // 用户翻页：滑动过渡
    }
    return;
  }
  const cur = flipCurrent();
  const step = doublePage ? 2 : 1;
  // 漫画卷尾：向后翻过最后一页 → 下一卷对话框
  if (dir > 0 && cur >= pages.length - step && Date.now() >= nextVolCooldownUntil) {
    if (showNextVolumeDialog()) return;
  }
  const target = Math.max(0, Math.min(cur + dir * step, pages.length - 1));
  scrollToPage(target, true);
  // 翻页过渡：给新页一个轻微淡入/缩放的“落页”效果
  if (target !== cur) {
    const el = stripEl.querySelectorAll('.page')[target];
    if (el) {
      el.classList.remove('turn');
      void el.offsetWidth; // 强制重排以重新触发动画
      el.classList.add('turn');
    }
  }
}

// EPUB 分页：把各章节里的图片提取成独立页
async function buildEpubFlipPages() {
  if (!stripEpubPath) return 0;
  flipEpubChapter = currentStripIndex();
  let data = null;
  try {
    data = await invoke('epub_pages', { path: stripEpubPath });
  } catch { /* 忽略 */ }
  flipEpub = data && data.paths ? data : { paths: [], chapter_offsets: [] };
  if (flipEpub.paths.length === 0) {
    pendingVol = null;
    return 0; // 纯文字章节：保持 iframe 视图
  }
  stripEl.innerHTML = '';
  pages = flipEpub.paths.map(p => ({ path: p, name: baseName(p), kind: 'img' }));
  for (const p of flipEpub.paths) {
    const img = document.createElement('img');
    img.className = 'page';
    img.decoding = 'async';
    img.loading = 'lazy';
    img.dataset.src = p;
    img.alt = '';
    stripEl.appendChild(img);
  }
  const io = new IntersectionObserver((ents) => {
    for (const en of ents) {
      if (!en.isIntersecting) continue;
      const el = en.target;
      if (!el.src) el.src = convertFileSrc(el.dataset.src);
      io.unobserve(el);
    }
  }, { root: stripEl, rootMargin: '600px 0px' });
  for (const img of stripEl.querySelectorAll('img')) io.observe(img);
  stripBuilt = true;
  const offs = flipEpub.chapter_offsets;
  let start;
  if (pendingVol && typeof pendingVol.page === 'number') {
    if (pendingVol.mode === 'flip') {
      start = pendingVol.page;
    } else {
      const ch = Math.min(pendingVol.page, Math.max(0, offs.length - 1));
      start = offs[ch] || 0;
    }
  } else {
    const ch = Math.min(flipEpubChapter, Math.max(0, offs.length - 1));
    start = offs[ch] || 0;
  }
  start = Math.min(start, Math.max(0, pages.length - 1));
  pendingVol = null;
  buildPageToc();
  scrollToPage(start);
  return start;
}

function chapterForImage(idx) {
  if (!flipEpub || !flipEpub.chapter_offsets) return 0;
  const offs = flipEpub.chapter_offsets;
  let ch = 0;
  for (let i = 0; i < offs.length; i++) {
    if (offs[i] <= idx) ch = i;
    else break;
  }
  return ch;
}

async function restoreEpubStrip(from) {
  const en = entries[listSel];
  if (!en || (!en.is_epub && !en.is_txt)) return;
  // 翻页视图占用了条漫状态，强制重建 iframe 章节视图
  stripBuilt = false;
  stripKind = '';
  await ensureEpubStrip(en);
  showPane('strip');
  let chapter, frac = 0;
  if (textBook) {
    if (from && typeof from.progress === 'number') {
      const p = progressToChapterWithin(from.progress);
      chapter = p.chapter; frac = p.within;
    } else if (typeof from === 'number') { chapter = from; }
    else if (from && typeof from.chapter === 'number') { chapter = from.chapter; frac = from.frac || 0; }
    else { chapter = 0; }
  } else {
    chapter = chapterForImage(typeof from === 'number' ? from : currentStripIndex());
  }
  const target = Math.min(chapter, Math.max(0, pages.length - 1));
  (holderFor(target) || stripEl.querySelector('.page') || stripEl).scrollIntoView({ block: 'start' });
  highlightIndex(target);
  buildPageToc();
  updatePageTocSel();
  // 文字书：章节内滚动进度等该章 iframe 高度就绪后再精确定位
  pendingScrollRestore = (textBook && frac > 0) ? { chapter: target, frac } : null;
}

// ---- 漫画页码面板（替代底部导航条：按钮 → 面板 → 页码网格）----

async function buildPageToc() {
  if (stripKind === 'epub' && textBook) { buildTextNav(); return; }
  tocListEl.innerHTML = '';
  if (pages.length === 0) return;
  const grid = document.createElement('div');
  grid.className = 'page-toc-grid';
  const frag = document.createDocumentFragment();
  for (let i = 0; i < pages.length; i++) {
    const b = document.createElement('button');
    b.className = 'page-toc-btn';
    b.dataset.idx = String(i);
    b.textContent = String(i + 1);
    b.addEventListener('click', () => {
      scrollToPage(i);
      closeTocPanel();
    });
    frag.appendChild(b);
  }
  grid.appendChild(frag);
  tocListEl.appendChild(grid);
  updatePageTocSel();
}

function updatePageTocSel() {
  if (textBook) return;
  const idx = currentStripIndex();
  const btns = tocListEl.querySelectorAll('.page-toc-btn');
  for (let i = 0; i < btns.length; i++) btns[i].classList.toggle('now', i === idx);
  const now = tocListEl.querySelector('.page-toc-btn.now');
  const grid = tocListEl.querySelector('.page-toc-grid');
  if (now && grid && tocPanelEl.classList.contains('show')) {
    tocListEl.scrollTop = grid.offsetTop + now.offsetTop - tocListEl.clientHeight / 2;
  }
}

stripEl.addEventListener('scroll', () => {
  hideReadingControlsNow(); // 滑动阅读时立即收起控件
  highlightIndex(currentStripIndex());
  updatePageTocSel();
  // 虚拟化：滚动模式跟随当前位置补/删占位 div（rAF 合并高频滚动）
  if (stripKind === 'epub' && textBook && !flipOn) {
    if (textHolderScrollRaf) cancelAnimationFrame(textHolderScrollRaf);
    textHolderScrollRaf = requestAnimationFrame(() => {
      textHolderScrollRaf = 0;
      syncTextHolders(scrollChapterIndex());
    });
  }
  if (focus === 'strip' && flipOn) {
    updateFlipIndicator();
    return;
  }
  const idx = currentStripIndex();
  progressEl.textContent = `${idx + 1} / ${pages.length} 页`;
  updateProgressBar();
  // 漫画卷尾：滚动模式滚到底部 → 下一卷对话框
  if (focus === 'strip' && !textBook) {
    if (stripEl.scrollTop > 0) stripUserScrolled = true;
    // 内容必须真的超出视口（图片未加载完时 scrollHeight 可能 ≤ clientHeight，不能算到底）
    const scrollable = stripEl.scrollHeight > stripEl.clientHeight + 8;
    const atBottom = scrollable &&
      stripEl.scrollTop + stripEl.clientHeight >= stripEl.scrollHeight - 4;
    if (atBottom && stripUserScrolled && nextVolDialogEl.hidden && Date.now() >= nextVolCooldownUntil) {
      showNextVolumeDialog();
    }
  }
}, { passive: true });

// 翻页模式：把一次横向滑动量累积成整页翻页（达到阈值翻一页，冷却防连翻）
function handleFlipWheel(dx) {
  const now = Date.now();
  if (now < flipCooldown) return; // 冷却中，忽略本次滑动
  flipWheelAccum += dx;
  const THRESHOLD = 40;
  let flipped = false;
  while (flipWheelAccum >= THRESHOLD) {
    flipWheelAccum -= THRESHOLD;
    flipTo(rtl ? -1 : 1);
    flipped = true;
  }
  while (flipWheelAccum <= -THRESHOLD) {
    flipWheelAccum += THRESHOLD;
    flipTo(rtl ? 1 : -1);
    flipped = true;
  }
  if (flipped) {
    flipWheelAccum = 0; // 翻一页后清空累积，避免同一次滑动连翻
    flipCooldown = now + 400; // 冷却延时（手机端减半，连翻更跟手），防止误翻
  }
}

// 触控板/鼠标滚轮左右翻页
stripEl.addEventListener('wheel', (e) => {
  if (!flipOn || focus !== 'strip') return;
  const dx = e.deltaX;
  const dy = e.deltaY;
  if (Math.abs(dx) <= Math.abs(dy)) return; // 纵向滑动不在这里处理
  e.preventDefault();
  handleFlipWheel(dx);
}, { passive: false });

// 手机触摸翻页：翻页模式下横向滑动 = 翻页（阈值/冷却与触控板一致）
let touchFlipX = 0, touchFlipY = 0, touchFlipLast = 0, touchFlipOn = false;
stripEl.addEventListener('touchstart', (e) => {
  if (!flipOn || focus !== 'strip') return;
  const t = e.touches[0];
  touchFlipX = t.clientX; touchFlipY = t.clientY; touchFlipLast = t.clientX;
  touchFlipOn = true;
}, { passive: true });
stripEl.addEventListener('touchmove', (e) => {
  if (!flipOn || focus !== 'strip' || !touchFlipOn) return;
  const t = e.touches[0];
  const dx = t.clientX - touchFlipLast;
  const dy = t.clientY - touchFlipY;
  touchFlipLast = t.clientX;
  if (Math.abs(dx) < 6 && Math.abs(dy) < 6) return;
  if (Math.abs(t.clientX - touchFlipX) < Math.abs(t.clientY - touchFlipY)) {
    touchFlipOn = false; // 纵向滑动：交给系统滚动
    return;
  }
  e.preventDefault();
  handleFlipWheel(-dx); // 手指左滑 = 下一页（直觉方向）
  hideReadingControlsNow(); // 滑动翻页立即收起控件
}, { passive: false });
stripEl.addEventListener('touchend', () => { touchFlipOn = false; });
stripEl.addEventListener('touchcancel', () => { touchFlipOn = false; });

// 窗口缩放时，翻页模式跟随视口重新适配并保持位置
let resizeRaf = 0;
window.addEventListener('resize', () => {
  if (focus !== 'strip' || !flipOn) return;
  if (resizeRaf) return;
  resizeRaf = requestAnimationFrame(() => {
    resizeRaf = 0;
    stripEl.style.scrollBehavior = 'auto'; // 缩放时不做平滑动画
    if (stripKind === 'epub' && textBook) {
      // 视口变化重排分栏：清空页数测量，只重测窗口内已加载章节，重新锚定到当前章节开头
      textChapterPages = textChapterPages.map(() => 0);
      updateCharsPerPageRef();
      rebuildTextOffsets();
      if (flipOn && textTotalPages !== textNavBuilt) buildTextNav();
      textWaitChapter = textCurChapter;
      textPendingChapter = null;
      textPendingAnimate = false; // resize 重排不用滑动过渡
      applyFlipMode();
      textShowChapter(textCurChapter, 0);
    } else {
      const target = doublePage ? Math.floor(flipAnchor / 2) * 2 : flipAnchor;
      applyFlipMode();
      scrollToPage(target);
    }
    stripEl.style.scrollBehavior = '';
  });
});

modeScrollBtn.addEventListener('click', () => setReadMode('scroll'));
modeFlipBtn.addEventListener('click', () => setReadMode('flip'));
pageSingleBtn.addEventListener('click', () => setPageMode('single'));
pageDoubleBtn.addEventListener('click', () => setPageMode('double'));
readerBackEl.appendChild(arrowLeftIconEl());
readerBackEl.addEventListener('click', () => exitStripMode());

// 图标化阅读控件
modeScrollBtn.title = '条漫（滚动）';
modeFlipBtn.title = '翻页';
pageSingleBtn.title = '单页';
pageDoubleBtn.title = '双页';
modeScrollBtn.textContent = '';
modeFlipBtn.textContent = '';
pageSingleBtn.textContent = '';
pageDoubleBtn.textContent = '';
modeScrollBtn.appendChild(scrollIconEl());
modeFlipBtn.appendChild(bookOpenIconEl());
pageSingleBtn.appendChild(rectIconEl());
pageDoubleBtn.appendChild(columnsIconEl());
themeBtnEl.appendChild(themeIconEl());
themeBtnEl.addEventListener('click', cycleReaderTheme);

// 文字书控件：目录抽屉与字号/字体
tocBtnEl.addEventListener('click', openTocPanel);
pageNavBtnEl.addEventListener('click', openTocPanel);
tocCloseBtn.addEventListener('click', closeTocPanel);
fontMinusBtn.addEventListener('click', () => changeFontSize(-1));
fontPlusBtn.addEventListener('click', () => changeFontSize(1));
fontFamilyBtn.addEventListener('click', cycleFontFamily);
// 点抽屉遮罩外关闭（点击目录面板外部区域）
tocPanelEl.addEventListener('click', (e) => {
  if (e.target === tocPanelEl) closeTocPanel();
});

// 阅读控件呼出：全屏轻点呼出（滑动翻页不呼出），静止 1.5s 后自动收起
let controlsHideTimer = 0;
function hideReadingControlsSoon(ms) {
  clearTimeout(controlsHideTimer);
  controlsHideTimer = setTimeout(() => {
    hideReadingControls();
  }, ms);
}
function hideReadingControlsNow() {
  clearTimeout(controlsHideTimer);
  hideReadingControls();
}
// 控件与标题栏/底部状态栏同步显隐
function showReadingControls() {
  modeTabEl.classList.add('show');
  pageModeEl.classList.add('show');
  themeBtnEl.classList.add('show');
  readerBackEl.classList.add('show');
  document.body.classList.add('ctl-on');
}
function hideReadingControls() {
  modeTabEl.classList.remove('show');
  pageModeEl.classList.remove('show');
  themeBtnEl.classList.remove('show');
  readerBackEl.classList.remove('show');
  document.body.classList.remove('ctl-on');
}
let touchActive = false; // 触摸手势进行中：抑制合成的鼠标事件，避免滑动翻页时呼出控件
window.addEventListener('touchstart', () => { touchActive = true; }, { passive: true });
window.addEventListener('touchend', () => { touchActive = false; });
window.addEventListener('touchcancel', () => { touchActive = false; });
window.addEventListener('mousemove', () => {
  if (focus !== 'strip') return;
  if (touchActive) return; // 触摸手势中的合成 mousemove 不算
  showReadingControls();
  hideReadingControlsSoon(3000);
});
// 轻点检测：位移 <10px 且 <500ms 才算轻点，才呼出控件
let tapStartX = 0, tapStartY = 0, tapStartT = 0, tapMoved = false;
window.addEventListener('touchstart', (e) => {
  const t = e.touches[0];
  tapStartX = t.clientX; tapStartY = t.clientY; tapStartT = Date.now(); tapMoved = false;
}, { passive: true });
window.addEventListener('touchmove', (e) => {
  const t = e.touches[0];
  if (Math.abs(t.clientX - tapStartX) > 10 || Math.abs(t.clientY - tapStartY) > 10) tapMoved = true;
}, { passive: true });
window.addEventListener('touchend', () => {
  if (focus !== 'strip') return;
  if (tapMoved || Date.now() - tapStartT > 500) return; // 滑动/长按不呼出
  showReadingControls();
  hideReadingControlsSoon(3000);
}, { passive: true });
modeTabEl.addEventListener('mouseenter', () => clearTimeout(controlsHideTimer));
pageModeEl.addEventListener('mouseenter', () => clearTimeout(controlsHideTimer));
themeBtnEl.addEventListener('mouseenter', () => clearTimeout(controlsHideTimer));
readerBackEl.addEventListener('mouseenter', () => clearTimeout(controlsHideTimer));
modeTabEl.addEventListener('mouseleave', () => hideReadingControlsSoon(150));
pageModeEl.addEventListener('mouseleave', () => hideReadingControlsSoon(150));
themeBtnEl.addEventListener('mouseleave', () => hideReadingControlsSoon(150));
readerBackEl.addEventListener('mouseleave', () => hideReadingControlsSoon(150));

// ---- 全屏书库网格（图标视图）----

const libGridBackEl = document.getElementById('lib-grid-back');
const libTitleEl = document.getElementById('lib-title');
const libResetEl = document.getElementById('lib-reset');
const libGridBodyEl = document.getElementById('list-grid');

let libGridMode = false;  // 图标视图是否激活
let libGridBook = null;   // 当前展开的书 {path,name}；null = 书页
let libGridCards = [];    // 书页卡片数据
let libGridVols = [];     // 卷页卡片数据
let libGridSel = 0;       // 当前选中卡片下标
let libGridOpening = false; // 防止双击焦点卡片连开两次
let libGridReadingReturn = null; // 从图标卷页进入阅读，退出时回到卷页 {book, volIndex}
let libGridLibEntries = []; // 打开图标视图时的书库条目快照（书页数据源）
let libGridBookReturn = false; // 散装书退出阅读后回到书页

let stripEntryPending = false; // 正在进入阅读（EPUB 解包阶段，用于显示顶部进度）
let unpackProgEl = null;
function showUnpackBar() {
  if (!unpackProgEl) {
    unpackProgEl = document.createElement('div');
    unpackProgEl.className = 'lib-thumb-progress';
    unpackProgEl.innerHTML =
      '<div class="lib-thumb-progress-bar"><div class="lib-thumb-progress-fill"></div></div>' +
      '<span class="lib-thumb-progress-text"></span>';
    document.body.appendChild(unpackProgEl);
  }
  unpackProgEl.hidden = false;
}
function updateUnpackBar(pct) {
  if (!unpackProgEl || unpackProgEl.hidden) return;
  unpackProgEl.querySelector('.lib-thumb-progress-fill').style.width =
    Math.min(100, Math.max(0, pct)) + '%';
  unpackProgEl.querySelector('.lib-thumb-progress-text').textContent =
    `正在解包 EPUB · ${Math.round(pct)}%`;
}
function hideUnpackBar() {
  if (unpackProgEl) unpackProgEl.hidden = true;
}

// ---- 文字书进入加载遮罩：盖住加载/定位过程，就绪后一次性显示 ----
const TEXT_LOADING_BG = { light: '#ffffff', sepia: '#f4ecd8', dark: '#14171c' };
let textLoadingEl = null;
function showTextLoading(label) {
  if (!textLoadingEl) {
    textLoadingEl = document.createElement('div');
    textLoadingEl.id = 'text-loading';
    textLoadingEl.innerHTML =
      '<div class="text-loading-box">' +
      '<div class="text-loading-bar"><div class="text-loading-fill"></div></div>' +
      '<div class="text-loading-label">正在加载章节…</div>' +
      '</div>';
    document.body.appendChild(textLoadingEl);
  }
  const lbl = textLoadingEl.querySelector('.text-loading-label');
  if (lbl) lbl.textContent = label || '正在加载章节…';
  textLoadingEl.style.background = TEXT_LOADING_BG[readerTheme] || '#ffffff';
  textLoadingEl.classList.remove('leaving');
  textLoadingEl.hidden = false;
}
function hideTextLoading() {
  if (!textLoadingEl || textLoadingEl.hidden) return;
  textLoadingEl.classList.add('leaving');
  setTimeout(() => {
    if (textLoadingEl) {
      textLoadingEl.classList.remove('leaving');
      textLoadingEl.hidden = true;
    }
  }, 260);
}
// 等目标章就绪：翻页模式 buildTextFlip 已 await 几何；滚动模式等章节高度（cshowH）就绪
function waitTextEntryReady() {
  return new Promise((resolve) => {
    if (!textBook || flipOn) { resolve(); return; }
    const idx = pendingScrollRestore ? pendingScrollRestore.chapter : currentStripIndex();
    const holder = holderFor(idx);
    const frame = holder && holder.querySelector('iframe');
    if (frame && frame.style && frame.style.height) { resolve(); return; }
    const timer = setTimeout(resolve, 800); // 超时兜底，不卡住进入
    const iv = setInterval(() => {
      const h = holderFor(idx) && holderFor(idx).querySelector('iframe');
      if (h && h.style && h.style.height) {
        clearInterval(iv);
        clearTimeout(timer);
        resolve();
      }
    }, 80);
  });
}
// 单/双页切换：等当前章按新列数重新测量完成
function waitPageModeReady(ch) {
  return new Promise((resolve) => {
    if ((textChapterPages[ch] || 0) > 0) { resolve(); return; }
    const timer = setTimeout(resolve, 3000);
    const iv = setInterval(() => {
      if ((textChapterPages[ch] || 0) > 0) { clearInterval(iv); clearTimeout(timer); resolve(); }
    }, 80);
  });
}

// 重新拉取当前书库条目（最近阅读排序随 last_read_at 更新）并渲染书页
async function refreshBookPage() {
  // 从卷页回退到书库：连读等场景退出后 cwd 可能还停在卷目录，
  // 统一先“进入书库目录”再渲染，而不是依赖返回上级目录的状态
  if (!favorites.some(f => f.path === cwd)) {
    const fav = favorites.find(f => cwd.startsWith(f.path + '/'));
    if (fav) cwd = fav.path;
  }
  if (favorites.some(f => f.path === cwd)) {
    try {
      entries = await invoke('list_dir', { path: cwd });
      images = entries.filter(e => e.is_image).map(e => e.path);
    } catch { /* 忽略 */ }
  }
  libGridLibEntries = entries.slice();
  libTitleEl.textContent = '书库';
  volCache.clear();
  await renderLibBookPage();
}

// 底部状态栏：书库/卷页统计信息
function updateGridStats() {
  if (libGridBook) {
    const t = readTimes[libGridBook.path] || 0;
    gridStatsEl.textContent = `${libGridVols.length} 卷 · 本书阅读 ${fmtDuration(t)}`;
  } else {
    let total = 0;
    for (const b of libGridCards) total += readTimes[b.path] || 0;
    gridStatsEl.textContent = `${libGridCards.length} 本 · 总阅读 ${fmtDuration(total)}`;
  }
}

// ---- 书籍元数据 + 标签筛选 + 阅读统计 ----

// 带缓存的电子书分卷读取（标签筛选重渲染时不重复读盘）
async function cachedVolumes(dir) {
  if (volCache.has(dir)) return volCache.get(dir);
  let vols = [];
  try {
    vols = await invoke('ebook_volumes', { dir });
  } catch { /* 忽略 */ }
  volCache.set(dir, vols);
  return vols;
}

// 批量拉取当前书库所有书籍的元数据，重建标签集合与标签栏
async function loadGridMeta() {
  const books = libGridLibEntries.filter(e => e.is_dir || e.is_pdf || e.is_epub || e.is_txt);
  const paths = books.map(e => e.path);
  if (paths.length === 0) {
    allTags = [];
    tagRefCount = new Map();
    renderTagBar();
    return;
  }
  try {
    const list = await invoke('list_book_meta', { paths });
    for (const m of list) {
      bookMetaMap[m.path] = m;
      readTimes[m.path] = m.read_time || 0;
    }
  } catch { /* 忽略 */ }
  // 有筛选在身时保留当前结果标签栏（applyTagFilter 会在筛完后重建），
  // 避免中间过程用全局规则把刚选中的单次引用标签清掉。
  if (activeTags.size === 0) {
    rebuildTagBarFrom(libGridLibEntries, 2);
  }
}

// 按给定书籍集合重建标签栏；minRef 为最少引用次数（全局 2，筛选后小集合用 1）
function rebuildTagBarFrom(entries, minRef) {
  const tagCount = new Map();
  for (const e of entries) {
    const tags = (bookMetaMap[e.path] && bookMetaMap[e.path].tags) || [];
    for (const t of tags) tagCount.set(t, (tagCount.get(t) || 0) + 1);
  }
  tagRefCount = tagCount;
  allTags = [...tagCount.entries()]
    .filter(([, n]) => n >= minRef)
    .sort((a, b) => b[1] - a[1] || a[0].localeCompare(b[0], 'zh'))
    .map(([t]) => t);
  const shown = new Set(allTags);
  for (const t of [...activeTags]) if (!shown.has(t)) activeTags.delete(t);
  renderTagBar();
}

function renderTagBar() {
  tagBarEl.innerHTML = '';
  if (allTags.length === 0) {
    tagBarEl.hidden = true;
    tagBarEl.style.maxHeight = '';
    return;
  }
  tagBarEl.hidden = false;
  const all = document.createElement('span');
  all.className = 'tag-filter' + (activeTags.size === 0 ? ' on' : '');
  all.textContent = '全部';
  all.addEventListener('click', () => {
    activeTags.clear();
    renderTagBar();
    applyTagFilter();
  });
  tagBarEl.appendChild(all);
  for (const t of allTags) {
    const chip = document.createElement('span');
    chip.className = 'tag-filter' + (activeTags.has(t) ? ' on' : '');
    chip.textContent = t;
    const n = tagRefCount.get(t) || 0;
    if (n > 0) chip.title = '被 ' + n + ' 本书引用';
    chip.addEventListener('click', () => {
      if (activeTags.has(t)) activeTags.delete(t);
      else activeTags.add(t);
      renderTagBar();
      applyTagFilter();
    });
    tagBarEl.appendChild(chip);
  }
  applyTagCollapse();
}

// 标签筛选栏：固定显示三行，超出部分在栏内滚动查看
function applyTagCollapse() {
  if (tagBarEl.hidden || tagBarEl.children.length === 0) {
    tagBarEl.style.maxHeight = '';
    return;
  }
  const tops = [...new Set(Array.from(tagBarEl.children).map(c => c.offsetTop))].sort((a, b) => a - b);
  if (tops.length <= 3) {
    tagBarEl.style.maxHeight = '';
    return;
  }
  const chip = tagBarEl.children[0];
  // 第三行底 + 底部 padding，正好显示三行
  tagBarEl.style.maxHeight = (tops[2] + chip.offsetHeight + 8) + 'px';
}
window.addEventListener('resize', () => {
  if (!tagBarEl.hidden) applyTagCollapse();
});

async function applyTagFilter() {
  const prev = libGridCards[libGridSel] ? libGridCards[libGridSel].path : null;
  await renderLibBookPage();
  // 选中标签后：标签栏切换为当前筛选结果里可用的标签合集（单次引用也显示）
  if (activeTags.size > 0 && libGridCards.length > 0) {
    rebuildTagBarFrom(libGridCards, 1);
  } else {
    rebuildTagBarFrom(libGridLibEntries, 2);
  }
  if (prev) {
    const idx = libGridCards.findIndex(b => b.path === prev);
    if (idx >= 0) {
      libGridSel = idx;
      updateLibSel();
    }
  }
}

// 把作者/评分/标签渲染到书卡底部（作者、星级、标签各占一行）
function renderCardMeta(b, card) {
  const el = card.querySelector('.card-meta');
  if (!el) return;
  const m = bookMetaMap[b.path] || {};
  el.innerHTML = '';
  const rating = Math.max(0, Math.min(5, Math.round(m.rating || 0)));
  const tags = m.tags || [];

  if (m.author) {
    const au = document.createElement('div');
    au.className = 'card-author';
    au.textContent = m.author;
    au.title = m.author;
    el.appendChild(au);
  }
  if (rating > 0) {
    const stars = document.createElement('div');
    stars.className = 'card-stars';
    stars.textContent = '★'.repeat(rating) + '☆'.repeat(5 - rating);
    stars.title = rating + ' 星';
    el.appendChild(stars);
  }
  if (tags.length > 0) {
    const row = document.createElement('div');
    row.className = 'card-tags';
    const shown = tags.slice(0, 3);
    for (const t of shown) {
      const chip = document.createElement('span');
      chip.className = 'tag-chip';
      chip.textContent = t;
      row.appendChild(chip);
    }
    if (tags.length > 3) {
      const more = document.createElement('span');
      more.className = 'tag-chip';
      more.textContent = '+' + (tags.length - 3);
      row.appendChild(more);
    }
    el.appendChild(row);
  }
}

// 封面左下角的进度胶囊（蓝底白字百分比）
function makeDonutEl() {
  const donut = document.createElement('span');
  donut.className = 'donut';
  donut.textContent = '0%';
  donut.title = '进度 0%';
  donut.hidden = true; // 未读（0%）不显示，加载到进度后再按需显示
  return donut;
}

function setDonut(donut, pct) {
  pct = Math.max(0, Math.min(100, Math.round(pct)));
  if (pct <= 0) {
    donut.hidden = true;
    donut.textContent = '0%';
    donut.title = '进度 0%';
    return;
  }
  donut.hidden = false;
  donut.textContent = pct + '%';
  donut.title = '进度 ' + pct + '%';
}

// 封面悬停备注提示：跟随鼠标，靠近视口边缘时翻转方向
function positionNoteTip(e) {
  const pad = 14;
  let x = e.clientX + pad;
  let y = e.clientY + pad;
  const r = hoverTipEl.getBoundingClientRect();
  if (x + r.width > window.innerWidth - 8) x = e.clientX - r.width - pad;
  if (y + r.height > window.innerHeight - 8) y = e.clientY - r.height - pad;
  hoverTipEl.style.left = x + 'px';
  hoverTipEl.style.top = y + 'px';
}

function hideNoteTip() {
  hoverTipEl.hidden = true;
}

// ---- 元数据编辑对话框 ----

function renderMetaStars() {
  metaStarsEl.innerHTML = '';
  for (let i = 1; i <= 5; i++) {
    const s = document.createElement('button');
    s.className = 'meta-star';
    s.type = 'button';
    s.title = i + ' 星';
    s.appendChild(starIconEl(i <= metaDialogRating));
    s.addEventListener('click', () => {
      metaDialogRating = i;
      renderMetaStars();
    });
    metaStarsEl.appendChild(s);
  }
  const clear = document.createElement('button');
  clear.className = 'btn meta-clear';
  clear.type = 'button';
  clear.textContent = '清除';
  clear.addEventListener('click', () => {
    metaDialogRating = 0;
    renderMetaStars();
  });
  metaStarsEl.appendChild(clear);
  if (metaDialogRating > 0 && metaDialogRating !== Math.round(metaDialogRating)) {
    metaRatingNoteEl.textContent = metaDialogRating.toFixed(1) + ' 星';
  } else {
    metaRatingNoteEl.textContent = '';
  }
}

function openMetaDialog(b) {
  metaDialogBook = b;
  metaPendingCover = null;
  const m = bookMetaMap[b.path] || {};
  metaTitleEl.value = m.title || '';
  metaAuthorEl.value = m.author || '';
  metaTagsEl.value = (m.tags || []).join(', ');
  metaNoteEl.value = m.note || '';
  metaDialogRating = Math.round(m.rating || 0);
  metaSmartStatusEl.hidden = true;
  metaSmartStatusEl.className = 'meta-smart-status';
  metaSmartStatusEl.textContent = '';
  fixTocArmed = false;
  metaFixTocEl.disabled = false;
  metaFixTocEl.textContent = '修复目录';
  metaFixStatusEl.textContent = '';
  metaCoverStatusEl.textContent = '';
  metaCoverPreviewEl.hidden = true;
  metaCoverPreviewEl.removeAttribute('src');
  metaCoverRemoveEl.hidden = true;
  (async () => {
    try {
      const c = await invoke('get_book_cover', { path: b.path });
      if (c && !metaPendingCover) {
        metaCoverPreviewEl.src = convertFileSrc(c);
        metaCoverPreviewEl.hidden = false;
        metaCoverRemoveEl.hidden = false;
      }
    } catch { /* 无自定义封面 */ }
  })();
  renderMetaStars();
  metaDialogEl.hidden = false;
  metaTitleEl.focus();
}

// 修复目录：先体检（只读），确认后重写坏书（自动备份原文件）
let fixTocArmed = false;
async function fixTocClick() {
  const b = metaDialogBook;
  if (!b) return;
  const btn = metaFixTocEl;
  const st = metaFixStatusEl;
  if (!fixTocArmed) {
    btn.disabled = true;
    st.textContent = '正在分析目录…';
    try {
      const r = await invoke('check_epub_toc', { path: b.path });
      if (r.needs_fix) {
        st.textContent =
          '发现 ' + r.mispointed + '/' + r.total + ' 条目录指向错误文件。' +
          '修复会重写本书为每章一文件（原文件自动备份），确认后再次点击「修复目录」。';
        fixTocArmed = true;
      } else {
        st.textContent = r.message || '目录结构正常，无需修复';
      }
    } catch (e) {
      st.textContent = '分析失败：' + (typeof e === 'string' ? e : JSON.stringify(e));
    } finally {
      btn.disabled = false;
    }
    return;
  }
  btn.disabled = true;
  st.textContent = '正在修复…（原文件将备份到工作目录 backups/）';
  try {
    const r = await invoke('fix_epub_toc', { path: b.path });
    st.textContent = r.message;
  } catch (e) {
    st.textContent = '修复失败：' + (typeof e === 'string' ? e : JSON.stringify(e));
  } finally {
    btn.disabled = false;
    fixTocArmed = false;
    btn.textContent = '修复目录';
  }
}

async function smartFetchMeta() {
  const b = metaDialogBook;
  if (!b || metaSmartEl.disabled) return;
  const btn = metaSmartEl;
  btn.disabled = true;
  btn.textContent = '获取中…';
  metaSmartStatusEl.hidden = false;
  metaSmartStatusEl.className = 'meta-smart-status';
  metaSmartStatusEl.textContent = '正在调用 AI 生成元数据（deepseek-v4-flash）…';
  try {
    const r = await invoke('smart_fetch_meta', { path: b.path });
    let filled = 0;
    if (r.title) { metaTitleEl.value = r.title; filled++; }
    if (r.author) { metaAuthorEl.value = r.author; filled++; }
    if (Array.isArray(r.tags) && r.tags.length) { metaTagsEl.value = r.tags.join(', '); filled++; }
    if (r.note) { metaNoteEl.value = r.note; filled++; }
    if (r.rating > 0) {
      metaDialogRating = Math.min(5, r.rating);
      filled++;
    }
    renderMetaStars();
    metaSmartStatusEl.className = 'meta-smart-status ok';
    metaSmartStatusEl.textContent =
      'AI 已填入 ' + filled + ' 项' +
      (r.message ? '（' + r.message + '）' : '') +
      '。请确认后点“保存”。';
  } catch (e) {
    metaSmartStatusEl.className = 'meta-smart-status err';
    metaSmartStatusEl.textContent = 'AI 填入失败：' + (typeof e === 'string' ? e : JSON.stringify(e));
  } finally {
    btn.disabled = false;
    btn.textContent = 'AI填入';
  }
}

function closeMetaDialog() {
  metaDialogEl.hidden = true;
  metaDialogBook = null;
  metaPendingCover = null; // 取消/关闭：丢弃未保存的封面改动
}

async function saveMetaDialog() {
  const b = metaDialogBook;
  if (!b) return;
  const title = metaTitleEl.value.trim();
  const author = metaAuthorEl.value.trim();
  const note = metaNoteEl.value.trim();
  const tags = metaTagsEl.value.split(/[,，、\s]+/).map(t => t.trim()).filter(Boolean);
  try {
    await invoke('set_book_meta', { path: b.path, title, author, rating: metaDialogRating, tags, note });
  } catch { /* 忽略 */ }
  try {
    if (metaPendingCover === 'remove') {
      await invoke('remove_book_cover', { path: b.path });
    } else if (metaPendingCover && typeof metaPendingCover === 'object') {
      await invoke('set_book_cover', {
        path: b.path,
        name: metaPendingCover.name,
        data: metaPendingCover.data,
      });
    }
  } catch (e) {
    metaCoverStatusEl.textContent = '封面保存失败：' + (typeof e === 'string' ? e : '未知错误');
    return;
  }
  metaPendingCover = null;
  bookMetaMap[b.path] = { title, author, rating: metaDialogRating, tags, note };
  closeMetaDialog();
  if (libGridMode && !libGridBook) {
    volCache.clear(); // 封面已变更：清卷数据缓存，重新读取最新缩略图
    await loadGridMeta();
    await renderLibBookPage();
  } else if (libGridBook) {
    volCache.delete(libGridBook.path);
    renderLibVolPage(libGridBook);
  }
}

// ---- 阅读统计对话框 ----

function fmtRelative(ts) {
  if (!ts) return '';
  const diff = Date.now() - ts * 1000;
  const min = Math.floor(diff / 60000);
  if (min < 1) return '刚刚';
  if (min < 60) return min + ' 分钟前';
  const hr = Math.floor(min / 60);
  if (hr < 24) return hr + ' 小时前';
  const day = Math.floor(hr / 24);
  if (day < 30) return day + ' 天前';
  return new Date(ts * 1000).toLocaleDateString();
}

async function openStatsDialog() {
  statsDialogEl.hidden = false;
  statsSummaryEl.innerHTML = '<div class="stats-empty">加载中…</div>';
  statsRecentEl.innerHTML = '';
  let stats = null;
  try {
    stats = await invoke('reading_stats');
  } catch { /* 忽略 */ }
  const timeSum = stats ? stats.total_read_time : 0;
  const total = stats ? stats.total_books : 0;
  const finished = stats ? stats.finished_books : 0;
  const reading = stats ? Math.max(0, stats.started_books - stats.finished_books) : 0;
  statsSummaryEl.innerHTML =
    '<div class="stat-card"><div class="stat-num">' + fmtDuration(timeSum) + '</div><div class="stat-label">总阅读时长</div></div>' +
    '<div class="stat-card"><div class="stat-num">' + finished + ' / ' + total + '</div><div class="stat-label">已读完</div></div>' +
    '<div class="stat-card"><div class="stat-num">' + reading + '</div><div class="stat-label">在读</div></div>';
  const recent = stats ? stats.recent : [];
  if (recent.length === 0) {
    statsRecentEl.innerHTML = '<div class="stats-empty">还没有阅读记录</div>';
    return;
  }
  const list = document.createElement('ul');
  list.className = 'stats-list';
  for (const r of recent) {
    const li = document.createElement('li');
    if (r.finished) {
      const done = document.createElement('span');
      done.className = 's-done';
      done.title = '已读完';
      done.appendChild(checkIconEl());
      li.appendChild(done);
    }
    const name = document.createElement('span');
    name.className = 's-name';
    name.textContent = cleanBookName(r.name);
    name.title = r.path;
    li.appendChild(name);
    const when = document.createElement('span');
    when.className = 's-when';
    when.textContent = fmtRelative(r.last_read_at);
    li.appendChild(when);
    const time = document.createElement('span');
    time.className = 's-time';
    time.textContent = fmtDuration(r.read_time || 0);
    li.appendChild(time);
    list.appendChild(li);
  }
  statsRecentEl.appendChild(list);
}

function closeStatsDialog() {
  statsDialogEl.hidden = true;
}

async function openLibGrid(book) {
  libGridMode = true;
  gridStatsEl.hidden = false;
  sidebarEl.classList.add('expanded'); // 侧栏扩展为全窗口
  libGridBodyEl.hidden = false;
  if (book) {
    return renderLibVolPage(book); // 直接进入卷页
  }
  // 兜底：当前目录不是任何书库时，保底进第一个可用的书库并刷新一次
  const favPaths = favorites.map(f => f.path);
  if (favorites.length > 0 && !favPaths.includes(cwd)) {
    for (const fav of favorites) {
      try {
        await loadDir(fav.path);
        break;
      } catch {
        /* 打不开的书库跳过，尝试下一个 */
      }
    }
  }
  await refreshBookPage();
  return null;
}

function closeLibGrid() {
  hideNoteTip();
  libGridMode = false;
  gridStatsEl.hidden = true;
  libStatsEl.hidden = true;
  tagBarEl.hidden = true;
  libGridBodyEl.hidden = true;
  libGridBackEl.hidden = true;
  libResetEl.hidden = true;
  libTitleEl.classList.remove('vol-title');
}

async function renderLibBookPage() {
  hideNoteTip();
  const prevPath = libGridBook ? libGridBook.path : null;
  libGridBook = null;
  libGridSel = 0;
  renderLibTabs();
  libGridBackEl.hidden = true;
  libResetEl.hidden = true;
  libTitleEl.classList.remove('vol-title');
  libGearEl.hidden = false;
  libStatsEl.hidden = false;
  await loadGridMeta();
  libGridBodyEl.innerHTML = '';
  if (favorites.length === 0) {
    // 未设置书库：不显示任何当前文件夹的文件
    libGridCards = [];
    updateGridStats();
    libGridBodyEl.innerHTML = '<div style="padding:40px;color:var(--muted)">请设置书库</div>';
    return;
  }
  let src = libGridLibEntries.filter(e => e.is_dir || e.is_pdf || e.is_epub || e.is_txt);
  const fav = favorites.find(f => f.path === cwd);
  if (fav && fav.hidden) {
    src = src.filter(e => !e.is_hidden); // 书库 eye-off：隐藏被标记的书
  }
  if (activeTags.size > 0) {
    src = src.filter(e => {
      const tags = (bookMetaMap[e.path] && bookMetaMap[e.path].tags) || [];
      return [...activeTags].every(t => tags.includes(t));
    });
  }
  libGridCards = src;
  updateGridStats();
  if (libGridCards.length === 0) {
    libGridBodyEl.innerHTML = activeTags.size > 0
      ? '<div style="padding:40px;color:var(--muted)">没有匹配此标签的书籍</div>'
      : '<div style="padding:40px;color:var(--muted)">没有书籍</div>';
    return;
  }
  for (const b of libGridCards) {
    const card = makeLibCard(b, b.is_dir ? () => openBookFromGrid(b) : () => openLooseBook(b));
    libGridBodyEl.appendChild(card);
    renderCardMeta(b, card);
    loadLibInfo(b, card);
  }
  // 从卷页返回书页时，焦点仍落在刚才那本书上
  if (prevPath) {
    const idx = libGridCards.findIndex(b => b.path === prevPath);
    if (idx >= 0) libGridSel = idx;
  }
  updateLibSel();
}

async function renderLibVolPage(book) {
  hideNoteTip();
  libGridBook = book;
  libGridSel = 0;
  libTabsEl.hidden = true;
  libTitleEl.hidden = false;
  libTitleEl.textContent = (bookMetaMap[book.path] && bookMetaMap[book.path].title) || cleanBookName(book.name);
  libTitleEl.classList.add('vol-title'); // 重置图标紧贴书名，不推到最右
  libEyeEl.hidden = true;
  libGearEl.hidden = true;
  libStatsEl.hidden = true;
  tagBarEl.hidden = true;
  // 返回按钮 = 当前书库胶囊（与书库页当前书库 tab 同款：图标 + 名称，蓝底白字）
  libGridBackEl.className = 'lib-tab on grid-back';
  libGridBackEl.textContent = '';
  const fav = activeLib();
  // 左箭头 + 书库图标 + 书库名
  const arr = arrowLeftIconEl();
  arr.setAttribute('width', '12');
  arr.setAttribute('height', '12');
  libGridBackEl.appendChild(arr);
  const ic = libraryIconEl(fav ? fav.icon : 'book-user');
  ic.setAttribute('width', '12');
  ic.setAttribute('height', '12');
  libGridBackEl.appendChild(ic);
  const lbl = document.createElement('span');
  lbl.textContent = fav ? (fav.alias || baseName(fav.path)) : '当前书库';
  libGridBackEl.appendChild(lbl);
  libGridBackEl.title = '返回书库';
  libGridBackEl.hidden = false;
  libResetEl.hidden = false;
  libResetEl.textContent = '';
  libResetEl.appendChild(resetIconEl());
  libGridBodyEl.innerHTML = '<div style="padding:40px;color:var(--muted)">加载中…</div>';
  volCache.delete(book.path); // 卷页必须反映最新进度，强制重新读取
  const vols = await cachedVolumes(book.path);
  libGridVols = vols;
  updateGridStats();
  libGridBodyEl.innerHTML = '';
  if (vols.length === 0) {
    libGridBodyEl.innerHTML = '<div style="padding:40px;color:var(--muted)">没有分卷</div>';
    return;
  }
  const holders = new Map();
  for (const v of vols) {
    libGridBodyEl.appendChild(makeVolCard(book, v, holders));
  }
  // 焦点自动落到最后一次阅读的卷
  const lastIdx = vols.findIndex(v => v.last_read);
  if (lastIdx >= 0) libGridSel = lastIdx;
  updateLibSel();
  // 缺失的缩略图按需生成（PDF 渲染首页 / EPUB 取封面），写回缓存
  if (holders.size > 0) await generateLibGridThumbs(holders);
}

let libThumbProg = null;
function showLibThumbProgress() {
  if (!libThumbProg) {
    libThumbProg = document.createElement('div');
    libThumbProg.className = 'lib-thumb-progress';
    libThumbProg.innerHTML =
      '<div class="lib-thumb-progress-bar"><div class="lib-thumb-progress-fill"></div></div>' +
      '<span class="lib-thumb-progress-text"></span>';
    document.body.appendChild(libThumbProg);
  }
  libThumbProg.hidden = false;
  updateLibThumbProgress(0, 1);
}
function updateLibThumbProgress(done, total) {
  if (!libThumbProg || libThumbProg.hidden) return;
  const pct = total > 0 ? Math.round((done / total) * 100) : 100;
  libThumbProg.querySelector('.lib-thumb-progress-fill').style.width = pct + '%';
  libThumbProg.querySelector('.lib-thumb-progress-text').textContent =
    `正在准备缩略图 ${done}/${total} · ${pct}%`;
}
function hideLibThumbProgress() {
  if (libThumbProg) libThumbProg.hidden = true;
}

async function generateLibGridThumbs(holders) {
  const total = holders.size;
  if (total === 0) return;
  showLibThumbProgress();
  let done = 0;
  for (const [vpath, holder] of holders) {
    const v = libGridVols.find(x => x.path === vpath);
    if (v) await generateVolumeThumb(v, holder);
    done++;
    updateLibThumbProgress(done, total);
  }
  hideLibThumbProgress();
}

function makeLibCard(b, onOpen) {
  const card = document.createElement('div');
  card.className = 'lib-card';
  card.dataset.path = b.path;
  const img = document.createElement('img');
  img.className = 'thumb';
  img.alt = '';
  const label = document.createElement('div');
  label.className = 'name';
  const s0 = splitBookName(b.name.replace(/\.(pdf|epub|txt)$/i, ''));
  const disp = b.is_dir
    ? cleanBookName(b.name)
    : (s0.title || s0.volume || b.name);
  const metaTitle = (bookMetaMap[b.path] && bookMetaMap[b.path].title) || '';
  label.textContent = metaTitle || disp;
  label.title = metaTitle || b.name;
  const cardMeta = document.createElement('div');
  cardMeta.className = 'card-meta';
  const eye = document.createElement('span');
  eye.className = 'eye-toggle' + (b.is_hidden ? ' off' : '');
  eye.title = b.is_hidden ? '显示' : '隐藏';
  eye.appendChild(eyeIconEl(b.is_hidden));
  eye.addEventListener('click', (ev) => {
    ev.stopPropagation();
    toggleLibBookEye(b, eye);
  });
  if (b.is_dir) {
    // 书文件夹：缩略图左上角刷新缓存按钮
    const refresh = document.createElement('span');
    refresh.className = 'refresh-btn';
    refresh.title = '刷新本书缓存';
    refresh.appendChild(refreshIconEl());
    refresh.addEventListener('click', (ev) => {
      ev.stopPropagation();
      refreshBookCache(b);
    });
    card.appendChild(refresh);
  }
  const thumbWrap = document.createElement('div');
  thumbWrap.className = 'thumb-wrap';
  thumbWrap.appendChild(img);
  thumbWrap.appendChild(makeDonutEl());
  // 散装电子书：缩略图左上角类型胶囊（TXT/EPUB/PDF）
  if (!b.is_dir) {
    const type = b.is_txt ? 'TXT' : (b.is_epub ? 'EPUB' : (b.is_pdf ? 'PDF' : ''));
    if (type) {
      const pill = document.createElement('span');
      pill.className = 'type-pill';
      pill.textContent = type;
      thumbWrap.appendChild(pill);
    }
  }
  thumbWrap.addEventListener('mouseenter', (e) => {
    const note = (bookMetaMap[b.path] && bookMetaMap[b.path].note) || '';
    if (note) {
      hoverTipEl.innerHTML = renderMarkdown(note);
      hoverTipEl.hidden = false;
      positionNoteTip(e);
    }
  });
  thumbWrap.addEventListener('mousemove', (e) => {
    if (!hoverTipEl.hidden) positionNoteTip(e);
  });
  thumbWrap.addEventListener('mouseleave', hideNoteTip);
  card.appendChild(thumbWrap);
  card.appendChild(label);
  card.appendChild(cardMeta);
  const edit = document.createElement('span');
  edit.className = 'edit-btn';
  edit.title = '编辑信息';
  edit.appendChild(pencilIconEl());
  edit.addEventListener('click', (ev) => {
    ev.stopPropagation();
    openMetaDialog(b);
  });
  card.appendChild(edit);
  card.appendChild(eye);
  card.addEventListener('click', () => {
    const i = libGridCards.indexOf(b);
    if (i < 0) return;
    if (IS_TOUCH || i === libGridSel) {
      if (libGridOpening) return; // 异步进入期间忽略重复点击
      libGridOpening = true;
      Promise.resolve(onOpen()).finally(() => { libGridOpening = false; });
    } else {
      libGridSel = i;
      updateLibSel();
    }
  });
  return card;
}

async function toggleLibBookEye(b, eyeEl) {
  let off = false;
  try {
    off = await invoke('toggle_eye', { path: b.path });
  } catch {
    return;
  }
  b.is_hidden = off;
  eyeEl.classList.toggle('off', off);
  eyeEl.title = off ? '显示' : '隐藏';
  eyeEl.textContent = '';
  eyeEl.appendChild(eyeIconEl(off));
  // 书库处于 eye-off 时，刚标记隐藏的书立即从网格消失
  if (off && libGridMode && !libGridBook) {
    const fav = favorites.find(f => f.path === cwd);
    if (fav && fav.hidden) renderLibBookPage();
  }
}

// 简单确认对话框，返回 Promise<boolean>
function confirmDialog(msg) {
  return new Promise((resolve) => {
    const box = document.createElement('div');
    box.className = 'confirm-box';
    box.innerHTML =
      `<div class="confirm-dialog"><p>${escapeHtml(msg)}</p>` +
      '<div class="confirm-actions">' +
      '<button class="btn confirm-no">取消</button>' +
      '<button class="btn btn-primary confirm-yes">确认</button>' +
      '</div></div>';
    document.body.appendChild(box);
    const close = (val) => {
      box.remove();
      resolve(val);
    };
    box.querySelector('.confirm-no').addEventListener('click', () => close(false));
    box.querySelector('.confirm-yes').addEventListener('click', () => close(true));
    box.addEventListener('click', (e) => {
      if (e.target === box) close(false);
    });
  });
}

// 密码输入对话框，返回输入值；取消返回 null
function passwordDialog(title, placeholder, confirmText) {
  return new Promise((resolve) => {
    const box = document.createElement('div');
    box.className = 'confirm-box';
    box.innerHTML =
      `<div class="confirm-dialog"><p>${escapeHtml(title)}</p>` +
      `<input class="pwd-input" type="password" placeholder="${escapeHtml(placeholder || '')}" autocomplete="off">` +
      '<div class="confirm-actions">' +
      '<button class="btn confirm-no">取消</button>' +
      `<button class="btn btn-primary confirm-yes">${escapeHtml(confirmText || '确认')}</button>` +
      '</div></div>';
    document.body.appendChild(box);
    const input = box.querySelector('.pwd-input');
    const close = (val) => {
      box.remove();
      resolve(val);
    };
    const confirm = () => close(input.value);
    box.querySelector('.confirm-no').addEventListener('click', () => close(null));
    box.querySelector('.confirm-yes').addEventListener('click', confirm);
    input.addEventListener('keydown', (e) => {
      if (e.key === 'Enter') confirm();
      else if (e.key === 'Escape') close(null);
      e.stopPropagation();
    });
    box.addEventListener('click', (e) => {
      if (e.target === box) close(null);
    });
    input.focus();
  });
}

// 简单提示对话框（只有一个确定按钮）
function showAlert(msg) {
  return new Promise((resolve) => {
    const box = document.createElement('div');
    box.className = 'confirm-box';
    box.innerHTML =
      `<div class="confirm-dialog"><p>${escapeHtml(msg)}</p>` +
      '<div class="confirm-actions">' +
      '<button class="btn btn-primary confirm-yes">确定</button>' +
      '</div></div>';
    document.body.appendChild(box);
    const close = () => {
      box.remove();
      resolve();
    };
    box.querySelector('.confirm-yes').addEventListener('click', close);
    box.addEventListener('click', (e) => {
      if (e.target === box) close();
    });
  });
}

// 刷新一本书（文件夹）的缓存：删旧缓存 → 重新生成封面与分卷缩略图
async function refreshBookCache(b) {
  if (!(await confirmDialog('是否刷新本书缓存？'))) return;
  try {
    await invoke('refresh_book_cache', { dir: b.path });
  } catch { /* 清理失败也继续生成 */ }
  const card = libGridBodyEl.querySelector('.lib-card[data-path="' + CSS.escape(b.path) + '"]');
  const imgEl = card ? card.querySelector('.thumb') : null;
  let vols = [];
  try {
    vols = await invoke('ebook_volumes', { dir: b.path });
  } catch { /* 忽略 */ }
  const missing = vols.filter(v => !v.thumb);
  if (missing.length > 0) {
    showLibThumbProgress();
    let done = 0;
    for (const v of missing) {
      await generateVolumeThumb(v, { img: imgEl, label: null });
      done++;
      updateLibThumbProgress(done, missing.length);
    }
    hideLibThumbProgress();
  }
  if (card) loadLibInfo(b, card);
}

async function loadLibInfo(b, card) {
  const imgEl = card.querySelector('.thumb');
  const donut = card.querySelector('.donut');
  if (!b.is_dir) {
    // 散装 epub/pdf：单卷书，封面 + 全书进度
    let cover = null;
    let pct = 0;
    try {
      const vols = await cachedVolumes(cwd);
      const v = vols.find(x => x.path === b.path);
      if (v) {
        if (v.thumb) cover = v.thumb;
        if (v.finished) pct = 100;
        else if (typeof v.saved_progress === 'number') pct = Math.min(100, Math.round(v.saved_progress * 100));
        else if (typeof v.saved_page === 'number' && v.total) {
          pct = Math.min(100, Math.round(((v.saved_page + 1) / v.total) * 100));
        }
      }
    } catch { /* 忽略 */ }
    if (!cover && b.is_epub) {
      try {
        const c = await invoke('epub_cover', { path: b.path });
        if (c) cover = c;
      } catch { /* 忽略 */ }
    } else if (!cover && b.is_pdf) {
      try {
        const c = await renderPdfThumb(b.path);
        if (c) cover = c;
      } catch { /* 忽略 */ }
    }
    if (cover) imgEl.src = convertFileSrc(cover);
    if (donut) setDonut(donut, pct);
    return;
  }
  try {
    const vols = await cachedVolumes(b.path);
    const last = vols.find(x => x.last_read);
    let cover = null;
    try {
      cover = await invoke('get_book_cover', { path: b.path }); // 书目录自定义封面优先
    } catch { /* 无 */ }
    if (!cover) cover = (last && last.thumb) || (vols[0] && vols[0].thumb);
    if (!cover) {
      // 重命名/移动导致缓存失效：按需解包取封面
      const cand = last || vols[0];
      if (cand && cand.kind === 'epub') {
        try {
          const c = await invoke('epub_cover', { path: cand.path });
          if (c) cover = c;
        } catch { /* 忽略 */ }
      } else if (cand && cand.kind === 'pdf') {
        try {
          const c = await renderPdfThumb(cand.path);
          if (c) cover = c;
        } catch { /* 忽略 */ }
      }
    }
    if (cover) imgEl.src = convertFileSrc(cover);
    let pct = 0;
    if (vols.length) {
      const frac = vols.reduce((acc, v) => {
        if (v.finished) return acc + 1;
        if (typeof v.saved_progress === 'number') {
          return acc + Math.min(1, v.saved_progress);
        }
        if (typeof v.saved_page === 'number' && v.total) {
          return acc + Math.min(1, (v.saved_page + 1) / v.total);
        }
        return acc;
      }, 0);
      pct = Math.round((frac / vols.length) * 100);
    }
    if (donut) setDonut(donut, pct);
  } catch { /* 忽略 */ }
}

function makeVolCard(book, v, holders) {
  const card = document.createElement('div');
  card.className = 'lib-card';
  const img = document.createElement('img');
  img.className = 'thumb';
  img.alt = '';
  if (v.thumb) img.src = convertFileSrc(v.thumb);
  const s = splitBookName(v.name.replace(/\.(pdf|epub)$/i, ''));
  const label = document.createElement('div');
  label.className = 'name';
  label.textContent = s.volume || v.name;
  label.title = v.name;
  if (!v.thumb && holders) holders.set(v.path, { img, label });
  // 缩略图容器：胶囊/进度类标记相对缩略图本身定位（底部 8px 指缩略图底部，而非含书名标签的卡片底部）
  const thumbWrap = document.createElement('div');
  thumbWrap.className = 'thumb-wrap';
  thumbWrap.appendChild(img);
  card.appendChild(thumbWrap);
  card.appendChild(label);
  if (v.finished) {
    // 已读完：缩略图左下角胶囊文字，不再用右上角勾
    const pill = document.createElement('span');
    pill.className = 'done-pill';
    pill.textContent = '已读完';
    pill.title = '已读完';
    thumbWrap.appendChild(pill);
  } else if (v.last_read) {
    // 最近阅读：缩略图右下角蓝底白字胶囊（右上角书签图标移除）
    const pill = document.createElement('span');
    pill.className = 'recent-pill';
    pill.textContent = '最近';
    pill.title = typeof v.saved_progress === 'number'
      ? `上次读到 ${Math.round(v.saved_progress * 100)}%`
      : (v.saved_page != null && v.total ? `上次读到第 ${v.saved_page + 1} 页` : '最近阅读');
    thumbWrap.appendChild(pill);
    card.classList.add('has-recent');
  }
  if (!v.finished && typeof v.saved_page === 'number' && v.total) {
    const prog = document.createElement('div');
    prog.className = 'prog';
    const fill = document.createElement('div');
    fill.className = 'prog-fill';
    fill.style.width = Math.min(100, Math.round(((v.saved_page + 1) / v.total) * 100)) + '%';
    prog.appendChild(fill);
    card.appendChild(prog);
  } else if (!v.finished && typeof v.saved_progress === 'number') {
    // 文字书：按内容百分比显示进度条
    const prog = document.createElement('div');
    prog.className = 'prog';
    const fill = document.createElement('div');
    fill.className = 'prog-fill';
    fill.style.width = Math.min(100, Math.round(v.saved_progress * 100)) + '%';
    prog.appendChild(fill);
    card.appendChild(prog);
  }
  card.addEventListener('click', () => {
    const i = libGridVols.indexOf(v);
    if (i < 0) return;
    if (IS_TOUCH || i === libGridSel) {
      openVolumeFromLibGrid(book, v); // 单击焦点项 = Enter 直接进入
    } else {
      libGridSel = i;
      updateLibSel();
    }
  });
  return card;
}

function openVolumeFromLibGrid(book, v) {
  libGridReadingReturn = {
    book: { path: book.path, name: book.name },
    volIndex: libGridVols.indexOf(v),
  };
  openVolume(book, v);
  closeLibGrid();
}

// 打开一本书（文件夹）：单卷书直接进阅读，多卷书进卷页
async function openBookFromGrid(b) {
  let vols = [];
  try {
    vols = await invoke('ebook_volumes', { dir: b.path });
  } catch { /* 忽略 */ }
  if (vols.length !== 1) {
    renderLibVolPage(b);
    return;
  }
  // 单卷书：直接进阅读；退出回到该书所在书库的书页并选中它
  libGridReadingReturn = {
    book: { path: b.path, name: b.name },
    volIndex: 0,
    direct: true,
    libPath: b.path.slice(0, b.path.lastIndexOf('/')),
  };
  openVolume(b, vols[0]);
  closeLibGrid();
}

// 散装 epub/pdf：点击直接进入阅读，退出后回到书页
async function openLooseBook(b) {
  const idx = entries.findIndex(e => e.path === b.path);
  if (idx < 0) return;
  listSel = idx;
  libGridBookReturn = true;
  closeLibGrid();
  if (b.is_epub || b.is_txt) await enterEpubStrip();
  else if (b.is_pdf) {
    // 散装 PDF 也恢复上次阅读位置与翻页模式
    try {
      const vols = await invoke('ebook_volumes', { dir: cwd });
      const v = vols.find(x => x.path === b.path);
      if (v) {
        pendingVol = {
          page: typeof v.saved_page === 'number' ? v.saved_page : null,
          mode: v.saved_mode || null,

          progress: typeof v.saved_progress === 'number' ? v.saved_progress : null,
        };
        await enterPdfStrip(v.saved_page);
        return;
      }
    } catch { /* 无记录 */ }
    await enterPdfStrip();
  }
}

function updateLibSel() {
  const cards = libGridBodyEl.children;
  const list = libGridBook ? libGridVols : libGridCards;
  libGridSel = Math.max(0, Math.min(libGridSel, list.length - 1));
  for (let i = 0; i < cards.length; i++) {
    cards[i].classList.toggle('sel', i === libGridSel);
  }
}

function moveLibSel(key) {
  const list = libGridBook ? libGridVols : libGridCards;
  if (list.length === 0) return;
  const body = libGridBodyEl;
  const cols = Math.max(1, Math.floor((body.clientWidth - 40 + 18) / (140 + 18)));
  let idx = libGridSel;
  if (key === 'ArrowRight') idx += 1;
  else if (key === 'ArrowLeft') idx -= 1;
  else if (key === 'ArrowDown') idx += cols;
  else if (key === 'ArrowUp') idx -= cols;
  libGridSel = Math.max(0, Math.min(idx, list.length - 1));
  updateLibSel();
  const card = libGridBodyEl.children[libGridSel];
  if (card) card.scrollIntoView({ block: 'nearest' });
}

libGridBackEl.addEventListener('click', () => refreshBookPage());

// 卷页：重置本书阅读记录（清空所有分卷进度/已读完/最近阅读，并刷新卷页）
libResetEl.addEventListener('click', async () => {
  if (!libGridBook) return;
  const ok = await confirmDialog('是否重置本书？确认后将清空所有分卷的阅读记录（进度、已读完、最近阅读）。');
  if (!ok) return;
  try {
    await invoke('reset_book_progress', { dir: libGridBook.path });
  } catch { /* 忽略 */ }
  volCache.delete(libGridBook.path);
  renderLibVolPage(libGridBook);
});

document.addEventListener('keydown', (e) => {
  const t = e.target;
  if (t && (t.tagName === 'INPUT' || t.tagName === 'TEXTAREA' || t.isContentEditable)) return;

  if (!libDialogEl.hidden) {
    if (e.key === 'Escape') closeLibDialog();
    return;
  }

  if (!metaDialogEl.hidden || !statsDialogEl.hidden) {
    return; // 元数据/统计对话框打开时不响应网格快捷键（Esc 由对话框自身处理）
  }

  if (libGridMode) {
    if (e.key === 'Escape') {
      if (libGridBook) {
        refreshBookPage();
      }
    } else if (e.key === 'Tab') {
      e.preventDefault();
      // 书库页：Tab 在多个书库之间切换（Shift+Tab 反向）
      if (!libGridBook && favorites.length > 1) {
        const cur = activeLib();
        const idx = favorites.findIndex(f => f.path === cur.path);
        const step = e.shiftKey ? -1 : 1;
        const next = favorites[(idx + step + favorites.length) % favorites.length];
        if (next && next.path !== cwd) {
          loadDir(next.path).then(async () => {
            libGridLibEntries = entries.slice();
            activeTags.clear();
            await renderLibBookPage();
            renderLibTabs();
          });
        }
      }
    } else if (e.key === 'Enter') {
      if (libGridBook) {
        const v = libGridVols[libGridSel];
        if (v) openVolumeFromLibGrid(libGridBook, v);
      } else {
        const b = libGridCards[libGridSel];
        if (!b) return;
        // 与双击一致：文件夹进卷页，散装 epub/pdf 直接进阅读
        if (b.is_dir) openBookFromGrid(b);
        else openLooseBook(b);
      }
    } else if (e.key === 'ArrowRight' || e.key === 'ArrowLeft' || e.key === 'ArrowUp' || e.key === 'ArrowDown') {
      e.preventDefault();
      moveLibSel(e.key);
    }
    return;
  }

  if (focus === 'strip') {
    if (!nextVolDialogEl.hidden) {
      // 下一卷对话框打开时：Enter=继续阅读，Esc=取消，其余按键不触发阅读动作
      if (e.key === 'Enter' || e.key === 'Escape') {
        e.preventDefault();
        if (e.key === 'Enter') continueNextVolume();
        else cancelNextVolume();
      }
      return;
    }
    if (e.key === 'Escape' && tocPanelEl.classList.contains('show')) {
      e.preventDefault();
      closeTocPanel();
      return;
    }
    if (e.key === 'Tab') {
      // 文字书：Tab 呼出/关闭目录
      if (stripKind === 'epub' && textBook) {
        e.preventDefault();
        if (tocPanelEl.classList.contains('show')) closeTocPanel();
        else openTocPanel();
      }
      return;
    }
    if (flipOn) {
      e.preventDefault();
      switch (e.key) {
        case 'ArrowRight': case 'l': flipTo(rtl ? -1 : 1); break;
        case 'ArrowLeft': case 'h': flipTo(rtl ? 1 : -1); break;
        case 'PageDown': case ' ': flipTo(1); break;
        case 'PageUp': flipTo(-1); break;
        case 'Home': if (stripKind === 'epub' && textBook) textLocate(0); else scrollToPage(0); break;
        case 'End': if (stripKind === 'epub' && textBook) textLocate(Math.max(0, textTotalPages - 1)); else scrollToPage(pages.length - 1); break;
        case 'ArrowUp': case 'k': stripEl.scrollBy({ top: -48 }); break;
        case 'ArrowDown': case 'j': stripEl.scrollBy({ top: 48 }); break;
        case 'Escape': case 'q': exitStripMode(); break;
        case 't': toggleRtl(); break;
      }
      return;
    }
    switch (e.key) {
      case 'ArrowUp': case 'k': e.preventDefault(); scrollStrip(-48); break;
      case 'ArrowDown': case 'j': e.preventDefault(); scrollStrip(48); break;
      case 'PageUp': e.preventDefault(); scrollStripPage(-1); break;
      case 'PageDown': case ' ': e.preventDefault(); scrollStripPage(1); break;
      case 'Home': e.preventDefault(); stripEl.scrollTop = 0; break;
      case 'End': e.preventDefault(); stripEl.scrollTop = stripEl.scrollHeight; break;
      case 'Escape': case 'q': {
        e.preventDefault();
        exitStripMode();
        break;
      }
      case 't': e.preventDefault(); toggleRtl(); break;
    }
    return;
  }

  switch (e.key) {
    case 'q': e.preventDefault(); savePosition(); invoke('quit_app'); break;
  }
});

(async () => {
  versionEl.textContent = 'cshow-gui v' + await invoke('app_version');
  await migrateLegacyStorage();
  // 恢复阅读背景主题（从后端配置目录读取）
  readerTheme = await loadReaderTheme();
  await loadReaderFont();
  updateFontButtons();
  applyReaderTheme(readerTheme);
  // 进入阅读时若正在解包 EPUB，显示顶部进度条
  try {
    const { listen } = window.__TAURI__.event;
    listen('epub-progress', (e) => {
      const p = e.payload && e.payload.percent;
      if (typeof p !== 'number') return;
      if (!stripEntryPending) return;
      showUnpackBar();
      updateUnpackBar(p);
      if (p >= 100) setTimeout(hideUnpackBar, 600);
    });
  } catch { /* 事件不可用时静默 */ }
  await refreshFavorites();
  const start = await invoke('initial_dir');
  await loadDir(start);
  openLibGrid(); // 应用始终为图标模式
})();
