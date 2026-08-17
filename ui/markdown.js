// 极简 Markdown 渲染：先把 HTML 转义，再转换常用语法（标题/列表/引用/代码块/粗体/斜体/删除线/链接）
function escapeHtml(s) {
  return s.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;').replace(/"/g, '&quot;');
}

function mdInline(s) {
  s = escapeHtml(s);
  s = s.replace(/`([^`]+)`/g, '<code>$1</code>');
  s = s.replace(/\*\*([^*]+)\*\*/g, '<strong>$1</strong>');
  s = s.replace(/__([^_]+)__/g, '<strong>$1</strong>');
  s = s.replace(/~~([^~]+)~~/g, '<del>$1</del>');
  s = s.replace(/\*([^*]+)\*/g, '<em>$1</em>');
  s = s.replace(/\[([^\]]+)\]\(([^)\s]+)\)/g, (m, t, u) => {
    if (/^(https?:\/\/|\/|#)/i.test(u)) {
      return '<a href="' + u + '" target="_blank" rel="noopener">' + t + '</a>';
    }
    return m;
  });
  return s;
}

export function renderMarkdown(md) {
  if (!md) return '';
  const lines = md.split('\n');
  const out = [];
  let inCode = false;
  let codeBuf = [];
  let listType = null;
  let paraBuf = [];

  const flushPara = () => {
    if (paraBuf.length) {
      out.push('<p>' + paraBuf.map(mdInline).join('<br>') + '</p>');
      paraBuf = [];
    }
  };
  const flushList = () => {
    if (listType) { out.push('</' + listType + '>'); listType = null; }
  };
  const openList = (type) => {
    if (listType !== type) { flushList(); out.push('<' + type + '>'); listType = type; }
  };

  for (const raw of lines) {
    if (/^\s*```/.test(raw)) {
      flushPara(); flushList();
      if (inCode) {
        out.push('<pre><code>' + escapeHtml(codeBuf.join('\n')) + '</code></pre>');
        codeBuf = [];
        inCode = false;
      } else {
        inCode = true;
      }
      continue;
    }
    if (inCode) { codeBuf.push(raw); continue; }

    if (/^\s*$/.test(raw)) { flushPara(); flushList(); continue; }

    const h = raw.match(/^(#{1,6})\s+(.*)$/);
    if (h) {
      flushPara(); flushList();
      const lvl = Math.min(6, h[1].length);
      out.push('<h' + lvl + '>' + mdInline(h[2]) + '</h' + lvl + '>');
      continue;
    }
    const ul = raw.match(/^\s*[-*]\s+(.*)$/);
    if (ul) { flushPara(); openList('ul'); out.push('<li>' + mdInline(ul[1]) + '</li>'); continue; }
    const ol = raw.match(/^\s*\d+[.)]\s+(.*)$/);
    if (ol) { flushPara(); openList('ol'); out.push('<li>' + mdInline(ol[1]) + '</li>'); continue; }
    const bq = raw.match(/^\s*>\s?(.*)$/);
    if (bq) { flushPara(); flushList(); out.push('<blockquote>' + mdInline(bq[1]) + '</blockquote>'); continue; }
    if (/^\s*([-*_])\s*(\1\s*){2,}$/.test(raw)) { flushPara(); flushList(); out.push('<hr>'); continue; }

    flushList();
    paraBuf.push(raw);
  }
  flushPara(); flushList();
  if (inCode) out.push('<pre><code>' + escapeHtml(codeBuf.join('\n')) + '</code></pre>');
  return out.join('');
}
