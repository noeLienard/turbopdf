/** The single-page UI served by `papier dev`. */
export const UI_HTML = /* html */ `<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>papier dev</title>
<style>
  :root { --bg:#f4f4f5; --card:#fff; --line:#e4e4e7; --text:#18181b; --muted:#71717a; --ok:#16a34a; --warn:#d97706; --err:#dc2626; }
  * { box-sizing:border-box; }
  body { margin:0; font:14px/1.5 -apple-system,BlinkMacSystemFont,"Segoe UI",Inter,sans-serif; background:var(--bg); color:var(--text); }
  header { position:sticky; top:0; z-index:10; display:flex; align-items:center; gap:14px; padding:10px 20px; background:rgba(255,255,255,.9); backdrop-filter:blur(8px); border-bottom:1px solid var(--line); }
  .logo { font-weight:700; letter-spacing:-.02em; }
  .file { color:var(--muted); font-family:ui-monospace,Menlo,monospace; font-size:12.5px; overflow:hidden; text-overflow:ellipsis; white-space:nowrap; }
  .grow { flex:1; min-width:0; }
  .pill { display:inline-flex; align-items:center; gap:6px; padding:3px 10px; border-radius:999px; border:1px solid var(--line); font-size:12px; color:var(--muted); background:#fff; }
  .dot { width:7px; height:7px; border-radius:50%; background:var(--muted); }
  .pill.ok .dot { background:var(--ok); } .pill.busy .dot { background:var(--warn); animation:pulse 1s infinite; } .pill.err .dot { background:var(--err); }
  .pill.warn { color:var(--warn); border-color:#fcd9a8; cursor:pointer; } .pill.warn .dot { background:var(--warn); }
  @keyframes pulse { 50% { opacity:.3; } }
  .btn { border:0; border-radius:8px; padding:7px 14px; font:inherit; font-weight:600; background:var(--text); color:#fff; text-decoration:none; cursor:pointer; }
  .btn.off { background:#d4d4d8; pointer-events:none; }
  main { padding:24px 20px 60px; display:flex; flex-direction:column; align-items:center; gap:22px; }
  .page { width:100%; max-width:820px; }
  .page img { display:block; width:100%; height:auto; background:#fff; border-radius:3px; box-shadow:0 1px 3px rgba(0,0,0,.08),0 8px 28px rgba(0,0,0,.08); }
  .page .label { margin:6px 2px 0; font-size:12px; color:var(--muted); text-align:right; }
  .banner { width:100%; max-width:820px; border-radius:10px; padding:12px 16px; background:#fef2f2; border:1px solid #fecaca; color:#991b1b; white-space:pre-wrap; font-family:ui-monospace,Menlo,monospace; font-size:13px; }
  #warnings { width:100%; max-width:820px; background:var(--card); border:1px solid var(--line); border-radius:10px; overflow:hidden; }
  #warnings h2 { margin:0; padding:10px 16px; font-size:13px; border-bottom:1px solid var(--line); background:#fffbeb; color:#92400e; }
  #warnings li { padding:10px 16px; border-bottom:1px solid var(--line); list-style:none; }
  #warnings li:last-child { border:0; }
  #warnings ul { margin:0; padding:0; }
  .tag { font:600 11px ui-monospace,Menlo,monospace; padding:2px 7px; border-radius:5px; background:#fef3c7; color:#92400e; margin-right:8px; }
  .empty { color:var(--muted); margin-top:80px; }
  [hidden] { display:none !important; }
</style>
</head>
<body>
<header>
  <span class="logo">papier</span>
  <span class="file grow" id="file"></span>
  <span class="pill busy" id="status"><span class="dot"></span><span id="statusText">Rendering…</span></span>
  <span class="pill" id="count" hidden></span>
  <span class="pill warn" id="warnPill" hidden><span class="dot"></span><span id="warnText"></span></span>
  <a class="btn off" id="download" href="/download.pdf">Download PDF</a>
</header>
<main id="main">
  <div class="banner" id="error" hidden></div>
  <div id="warnings" hidden><h2>Warnings</h2><ul id="warnList"></ul></div>
  <div id="pages"></div>
  <div class="empty" id="empty">Rendering your document…</div>
</main>
<script>
const $ = (id) => document.getElementById(id);
let rev = -1;

function pill(cls, text) { $('status').className = 'pill ' + cls; $('statusText').textContent = text; }

async function refresh() {
  const s = await (await fetch('/state')).json();
  $('file').textContent = s.file;
  if (s.status === 'rendering') { pill('busy', 'Rendering…'); return; }
  rev = s.rev;

  $('error').hidden = s.status !== 'error';
  if (s.status === 'error') {
    $('error').textContent = s.error;
    pill('err', 'Error');
  } else {
    pill('ok', 'Up to date');
  }
  $('download').classList.toggle('off', !s.hasPdf);

  const wl = $('warnList'); wl.innerHTML = '';
  for (const w of s.warnings) {
    const li = document.createElement('li');
    const tag = document.createElement('span'); tag.className = 'tag'; tag.textContent = w.type;
    li.append(tag, document.createTextNode(w.message));
    wl.append(li);
  }
  $('warnings').hidden = !s.warnings.length;
  $('warnPill').hidden = !s.warnings.length;
  $('warnText').textContent = s.warnings.length + (s.warnings.length > 1 ? ' warnings' : ' warning');

  const c = $('count'); c.hidden = !s.count; c.textContent = s.count + (s.count > 1 ? ' pages' : ' page');
  $('empty').hidden = s.count > 0;

  const box = $('pages');
  while (box.children.length > s.count) box.lastChild.remove();
  for (let i = 1; i <= s.count; i++) {
    let el = box.children[i - 1];
    if (!el) {
      el = document.createElement('div'); el.className = 'page';
      el.innerHTML = '<img alt=""><div class="label"></div>';
      box.append(el); el.style.marginBottom = '22px';
    }
    el.querySelector('.label').textContent = 'Page ' + i + ' / ' + s.count;
    const next = new Image();
    next.onload = () => { el.querySelector('img').src = next.src; };
    next.src = '/page/' + i + '.png?rev=' + s.rev;
  }
}

const es = new EventSource('/events');
es.onmessage = () => refresh();
$('warnPill').onclick = () => $('warnings').scrollIntoView({ behavior: 'smooth' });
refresh();
</script>
</body>
</html>`;
