"use strict";
const T = window.__TAURI__;
const invoke = T.core.invoke;
const listen = T.event.listen;
const win = T.window.getCurrentWindow();
const $ = (id) => document.getElementById(id);

const S = { mode: "join", action: "join", scan: null, set: null, file: null, done: null, info: null };

// ---------- עזרים ----------
function human(n) {
  const u = ["B", "KB", "MB", "GB", "TB"]; let v = n, i = 0;
  while (v >= 1024 && i < u.length - 1) { v /= 1024; i++; }
  const t = i === 0 ? `${n} B` : `${v.toFixed(v >= 100 ? 0 : 1)} ${u[i]}`;
  return `\u2066${t}\u2069`; // מבודד כיוון: המספר והיחידה לא מתהפכים בתוך טקסט עברי
}
function dur(sec) {
  sec = Math.max(0, Math.round(sec));
  if (sec < 60) return `${sec} שניות`;
  const m = Math.floor(sec / 60), s = sec % 60;
  if (m < 60) return `${m} דקות${s ? ` ו-${s} שניות` : ""}`;
  return `${Math.floor(m / 60)} שעות ו-${m % 60} דקות`;
}
function el(tag, cls, text) { const e = document.createElement(tag); if (cls) e.className = cls; if (text != null) e.textContent = text; return e; }
function ltr(text) { const s = el("span", "ltr", text); return s; }
// קודי הגליפים בפונט Fluent System Icons (תת-קבוצה)
const IC = { archive: 0xf436, parts: 0xf4d3, folder: 0xf42f, options: 0xf588, sync: 0xf191, box: 0xe1bc, warn: 0xf86a, check: 0xf295 };
function glyph(code, filled) { const i = el("i", "fi" + (filled ? " fif" : ""), String.fromCodePoint(code)); return i; }
// סדר הכרטיס כמו במסייע (מימין לשמאל): סימון (אם יש), אריח אייקון, טקסט.
function card({ icon, label, value, sub, side, ltrValue, cls, mark }) {
  const c = el("div", "card " + (cls || ""));
  if (mark) c.append(el("div", "mark radio"));
  const tile = el("div", "tile"); tile.append(glyph(icon || IC.archive, icon === IC.check)); c.append(tile);
  const ct = el("div", "ct");
  if (label) ct.append(el("div", "l", label));
  const v = el("div", "v"); if (ltrValue) v.append(ltr(value)); else v.textContent = value; ct.append(v);
  if (sub) { const sd = el("div", "s"); (Array.isArray(sub) ? sub : [sub]).forEach((p) => sd.append(typeof p === "string" ? document.createTextNode(p) : ltr(p.n))); ct.append(sd); }
  if (side) ct.append(el("div", "f", side));
  c.append(ct);
  return c;
}
function stepLabel(n, total) { return `שלב ${n} מתוך ${total}`; }
function show(id) {
  document.querySelectorAll(".screen").forEach((s) => (s.hidden = s.id !== id));
  const total = S.mode === "join" ? 4 : 3;
  const map = S.mode === "join"
    ? { "s-pick": 1, "s-action": 2, "s-ready": 3, "s-progress": 4 }
    : { "s-cpick": 1, "s-ready": 2, "s-progress": 3 };
  const step = map[id];
  document.querySelectorAll(`#${id} .dots`).forEach((d) => {
    d.textContent = "";
    for (let i = 1; i <= total; i++) d.append(el("i", i === step ? "on" : ""));
  });
  document.querySelectorAll(`#${id} .steplabel`).forEach((l) => (l.textContent = step ? stepLabel(step, total) : ""));
}
document.querySelectorAll("[data-back]").forEach((b) => b.addEventListener("click", () => show(b.dataset.back)));

// ---------- חלון ----------
$("btn-min").onclick = () => win.minimize();
$("btn-close").onclick = () => win.close();

// ---------- הרכבה: בחירה ----------
async function loadScan(path) {
  try {
    S.scan = path ? await invoke("scan_dir", { path }) : await invoke("scan");
  } catch (e) { return fail(String(e)); }
  renderSets();
  show("s-pick");
}
function renderSets() {
  const list = $("set-list"); list.textContent = "";
  S.set = null; $("pick-next").disabled = true;
  const { sets, dir } = S.scan;
  const h = $("pick-hint"); h.textContent = "נסרקה התיקייה: "; h.append(ltr(dir));
  if (!sets.length) {
    list.append(card({ icon: IC.warn, value: "לא נמצאו חלקי ספרייה", sub: "הנח את הקובץ הזה ליד קבצי part-000 ו-part-001, או בחר תיקייה אחרת." }));
    list.append(otherFolderButton());
    return;
  }
  sets.forEach((s) => {
    const sub = s.problem ? s.problem
      : `${s.parts.length} חלקים · ${human(s.total)} · ${s.hasManifest ? "מניפסט נמצא ✓" : "ללא מניפסט"}`;
    const c = card({ icon: IC.archive, value: s.archive, ltrValue: true, sub, cls: s.problem ? "bad" : "pick", mark: !s.problem });
    if (!s.problem) {
      c.onclick = () => {
        list.querySelectorAll(".card").forEach((x) => x.classList.remove("sel"));
        c.classList.add("sel"); S.set = s; $("pick-next").disabled = false;
      };
    }
    list.append(c);
  });
  const ok = sets.filter((s) => !s.problem);
  list.append(otherFolderButton());
  if (ok.length === 1) list.querySelector(".card.pick").click();
}
$("go-join").onclick = () => { S.mode = "join"; loadScan(); };
function otherFolderButton() {
  const b = el("button", "btn tonal wide", "בחירת תיקייה אחרת…");
  b.onclick = async () => { const p = await invoke("pick_folder"); if (p) loadScan(p); };
  return b;
}
$("pick-next").onclick = () => renderActions();

// ---------- מה לעשות: הרכבה או חילוץ ישר ----------
function renderActions() {
  const s = S.set, list = $("action-list"); list.textContent = "";
  const canExtract = !!s.extractName;
  S.action = "join";
  const opts = [
    { id: "join", icon: IC.archive, value: "הרכבה לקובץ אחד", ltrName: s.archive,
      sub: ["מחבר את החלקים אל ", { n: s.archive }, " ומאמת. מהיר, וחוסך מקום."], side: `מקום נדרש: ${human(s.total)}` },
    { id: "extract", icon: IC.parts, value: "חילוץ ישר לקובץ DB", ltrName: s.extractName,
      sub: canExtract ? ["מחבר ופותח בבת אחת אל ", { n: s.extractName }, ", בלי קובץ ביניים."] : "זמין רק לקבצי zst.",
      side: canExtract ? `מקום נדרש: ${s.contentSize ? human(s.contentSize) : "כ-" + human(s.total * 4)}` : null, disabled: !canExtract },
  ];
  opts.forEach((o) => {
    const c = card({ icon: o.icon, value: o.value, sub: o.sub, side: o.side, cls: o.disabled ? "bad" : "pick", mark: !o.disabled });
    if (o.disabled) c.classList.remove("bad"), (c.style.opacity = ".55");
    else c.onclick = () => {
      list.querySelectorAll(".card").forEach((x) => x.classList.remove("sel"));
      c.classList.add("sel"); S.action = o.id;
    };
    list.append(c);
  });
  list.querySelector(".card.pick").click();
  show("s-action");
}
$("action-next").onclick = () => renderReady();

// ---------- דחיסה: בחירה ----------
$("go-compress").onclick = () => { S.mode = "compress"; S.file = null; renderCFile(); show("s-cpick"); };
function renderCFile() {
  const box = $("cfile"); box.textContent = "";
  $("cpick-next").disabled = !S.file;
  if (!S.file) box.append(card({ icon: IC.warn, value: "עדיין לא נבחר קובץ" }));
  else box.append(card({ icon: IC.archive, value: S.file.name, ltrValue: true, sub: human(S.file.size), cls: "sel", mark: true }));
  const b = el("button", "btn tonal wide", S.file ? "בחירת קובץ אחר…" : "בחירת קובץ…");
  b.onclick = async () => { const f = await invoke("pick_file"); if (f) { S.file = f; renderCFile(); } };
  box.append(b);
}
$("cpick-next").onclick = () => renderReady();

// ---------- מוכנים ----------
function addDelCheck(box, enabled) {
  if (!enabled) return;
  const l = el("label", "check"); const i = document.createElement("input"); i.type = "checkbox";
  i.onchange = () => (S.delAfter = i.checked);
  l.append(i, el("span", null, "מחק את החלקים אחרי הרכבה מוצלחת ומאומתת")); box.append(l);
}
function renderReady() {
  const box = $("ready-cards"); box.textContent = "";
  const alert = el("div", "alert"); alert.hidden = true;
  let canStart = true, free, need, outDir;
  S.overwrite = false;
  if (S.mode === "join") {
    const s = S.set, ex = S.action === "extract";
    free = S.scan.free; outDir = S.scan.outDir;
    const outName = ex ? s.extractName : s.archive;
    need = (ex ? (s.contentSize || s.total * 4) : s.total) + (64 << 20);
    $("ready-title").textContent = ex ? "מוכנים לחלץ" : "מוכנים להרכיב";
    $("ready-hint").textContent = "בדוק שהכל נראה כמו שצריך ולחץ על התחל.";
    box.append(card({ icon: IC.archive, label: "ייווצר הקובץ", value: outName, ltrValue: true, sub: ex && s.contentSize ? human(s.contentSize) : null }));
    box.append(card({ icon: IC.parts, label: "חלקים", value: `${s.parts.length} חלקים · ${human(s.total)}` }));
    box.append(card({ icon: IC.check, label: "אימות",
      value: s.hasManifest ? "SHA-256 מול המניפסט" : "אין מניפסט — לא ניתן לאמת",
      sub: ex ? "וגם סכום הביקורת הפנימי של zstd" : null }));
    S.overwrite = ex ? s.extractExists : s.existsInOut;
    if (S.overwrite) note(alert, `הקובץ ${outName} כבר קיים בתיקייה ויוחלף.`, true);
    else if (!s.hasManifest) note(alert, `לא נמצא קובץ מניפסט, ה${ex ? "חילוץ" : "הרכבה"} יתבצע בלי אימות מול המניפסט. מומלץ לשים אותו ליד החלקים.`, true);
    S.delAfter = false; addDelCheck(box, s.hasManifest);
    $("start").textContent = ex ? "התחל חילוץ" : "התחל הרכבה";
  } else {
    const f = S.file; free = f.free; outDir = f.outDir; need = f.size + (64 << 20);
    $("ready-title").textContent = "מוכנים לדחוס";
    $("ready-hint").textContent = "הדחיסה זהה לבניית הספרייה: zstd ברמה 19.";
    box.append(card({ icon: IC.archive, label: "קובץ מקור", value: f.name, ltrValue: true, sub: human(f.size) }));
    box.append(card({ icon: IC.parts, label: "ייווצר", value: f.name + ".zst", ltrValue: true }));
    box.append(card({ icon: IC.options, label: "דחיסה", value: "zstd רמה 19 · חלון 8MB · סכום ביקורת" }));
    S.overwrite = f.existsInOut;
    if (f.existsInOut) note(alert, `הקובץ ${f.name}.zst כבר קיים בתיקייה ויוחלף.`, true);
    S.delAfter = false;
    $("start").textContent = "התחל דחיסה";
  }
  box.append(card({ icon: IC.folder, label: "תיקיית הפלט (היחידה שנכתבת)", value: outDir, ltrValue: true,
    sub: free != null ? `מקום פנוי: ${human(free)}` : null }));
  if (free != null && free < need) {
    note(alert, `אין מספיק מקום פנוי בתיקייה: נדרשים ${human(need)} ופנויים ${human(free)}.`, false);
    canStart = false;
  }
  box.append(alert);
  $("start").disabled = !canStart;
  show("s-ready");
}
function note(a, text, warn) { a.textContent = text; a.hidden = false; a.className = "alert" + (warn ? " warn" : ""); }
$("ready-back").onclick = () => show(S.mode === "join" ? "s-action" : "s-cpick");

// ---------- ריצה ----------
$("start").onclick = async () => {
  $("pct").textContent = "0%"; $("bar-fill").style.width = "0%";
  $("prog-meta").textContent = ""; $("prog-size").textContent = ""; $("prog-speed").textContent = "";
  $("stop").disabled = false;
  if (S.mode === "join" && S.action === "extract") {
    $("prog-title").textContent = "מחלץ את הספרייה…"; $("prog-sub").textContent = "מחבר ופותח בבת אחת, בלי קובץ ביניים.";
  } else if (S.mode === "join") {
    $("prog-title").textContent = "מרכיב את הספרייה…"; $("prog-sub").textContent = "אפשר לעצור בכל רגע, לא יישאר קובץ חלקי.";
  } else {
    $("prog-title").textContent = "דוחס את הקובץ…"; $("prog-sub").textContent = "רמה 19 איטית בכוונה, כדי להגיע לאותו גודל כמו הספרייה.";
  }
  show("s-progress");
  try {
    if (S.mode === "join") await invoke("start_join", { dir: S.set.dir, archive: S.set.archive, overwrite: S.overwrite, action: S.action });
    else await invoke("start_compress", { path: S.file.path, overwrite: S.overwrite });
  } catch (e) { fail(String(e)); }
};
$("stop").onclick = () => { $("stop").disabled = true; invoke("cancel"); };

listen("progress", (ev) => {
  const p = ev.payload; const pct = p.total ? (p.done / p.total) * 100 : 0;
  $("pct").textContent = `${Math.floor(pct)}%`; $("bar-fill").style.width = `${pct}%`;
  if (p.phase === "join") $("prog-meta").textContent = `מחבר חלק ${p.partIndex + 1} מתוך ${p.partCount}`;
  else if (p.phase === "extract") $("prog-meta").textContent = `מחלץ מחלק ${p.partIndex + 1} מתוך ${p.partCount}`;
  else $("prog-meta").textContent = "דוחס…";
  const eta = p.speed > 0 ? (p.total - p.done) / p.speed : 0;
  $("prog-speed").textContent = `${human(p.speed)} בשנייה · נותרו כ-${dur(eta)}`;
  $("prog-size").textContent = `${human(p.done)} מתוך ${human(p.total)}`;
});
listen("finished", async (ev) => {
  const d = (S.done = ev.payload);
  const box = $("done-cards"); box.textContent = "";
  box.append(card({ icon: IC.archive, label: "קובץ", value: d.name, ltrValue: true }));
  if (d.kind === "join" || d.kind === "extract") {
    box.append(card({ icon: IC.parts, label: "גודל", value: human(d.size) }));
    box.append(card({ icon: IC.check, label: "אימות", value: d.verified ? "אומת מול המניפסט ✓" : "לא אומת (אין מניפסט)", sub: d.sha256 ? "\u2066SHA-256: " + d.sha256.slice(0, 16) + "…\u2069" : null }));
  } else {
    box.append(card({ icon: IC.parts, label: "גודל", value: `${human(d.inputSize)} ← ${human(d.size)}` }));
  }
  box.append(card({ icon: IC.sync, label: "משך הפעולה", value: dur(d.seconds) }));
  if (d.partsDeletable && S.mode === "join") {
    const b = el("button", "btn tonal wide", d.kind === "extract" ? "מחק את החלקים (נשמר רק קובץ ה-DB)" : "מחק את החלקים (נשמר רק הקובץ המורכב)");
    b.id = "del-now";
    b.onclick = () => { if (b.dataset.armed) return deleteParts(); b.dataset.armed = "1"; b.textContent = "לחץ שוב לאישור המחיקה"; };
    box.append(b);
  }
  show("s-done");
  if (S.mode === "join" && d.partsDeletable && S.delAfter) await deleteParts();
});
listen("failed", (ev) => {
  const { cancelled, message } = ev.payload;
  $("err-badge").className = "badge " + (cancelled ? "warn" : "bad");
  $("err-title").textContent = cancelled ? "הפעולה נעצרה" : "משהו השתבש";
  $("err-msg").textContent = message; show("s-error");
});
function fail(msg) { $("err-badge").className = "badge bad"; $("err-title").textContent = "משהו השתבש"; $("err-msg").textContent = msg; show("s-error"); }

async function deleteParts() {
  const b = $("del-now") || el("button");
  try { const n = await invoke("delete_parts", { dir: S.set.dir, archive: S.set.archive, action: S.action }); b.textContent = `נמחקו ${n} קבצים`; }
  catch (e) { b.textContent = String(e); }
  b.disabled = true;
}
$("reveal").onclick = () => S.done && invoke("reveal", { path: S.done.output });
$("finish").onclick = () => win.close();
$("retry").onclick = () => ((S.mode === "join" ? S.set : S.file) ? renderReady() : show("s-welcome"));
$("err-close").onclick = () => show("s-welcome");
