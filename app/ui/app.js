"use strict";
const T = window.__TAURI__;
const invoke = T.core.invoke;
const listen = T.event.listen;
const win = T.window.getCurrentWindow();
const $ = (id) => document.getElementById(id);

const S = { mode: "join", scan: null, set: null, file: null, done: null, info: null };

// ---------- עזרים ----------
function human(n) {
  const u = ["B", "KB", "MB", "GB", "TB"]; let v = n, i = 0;
  while (v >= 1024 && i < u.length - 1) { v /= 1024; i++; }
  return i === 0 ? `${n} B` : `${v.toFixed(v >= 100 ? 0 : 1)} ${u[i]}`;
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
function card({ icon, label, value, sub, ltrValue, cls }) {
  const c = el("div", "card " + (cls || ""));
  c.append(el("div", "tile", icon || "•"));
  const ct = el("div", "ct");
  if (label) ct.append(el("div", "l", label));
  const v = el("div", "v"); if (ltrValue) v.append(ltr(value)); else v.textContent = value; ct.append(v);
  if (sub) ct.append(el("div", "s", sub));
  c.append(ct); return c;
}
function show(id) {
  document.querySelectorAll(".screen").forEach((s) => (s.hidden = s.id !== id));
  document.querySelectorAll(`#${id} .dots`).forEach((d) => {
    const step = +d.dataset.step; d.textContent = "";
    for (let i = 1; i <= 3; i++) d.append(el("i", i === step ? "on" : ""));
  });
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
    list.append(card({ icon: "?", value: "לא נמצאו חלקי ספרייה", sub: "הנח את הקובץ הזה ליד קבצי part-000 ו-part-001, או בחר תיקייה אחרת." }));
    return;
  }
  sets.forEach((s) => {
    const sub = s.problem ? s.problem
      : `${s.parts.length} חלקים · ${human(s.total)} · ${s.hasManifest ? "מניפסט נמצא ✓" : "ללא מניפסט"}`;
    const c = card({ icon: "▤", value: s.archive, ltrValue: true, sub, cls: s.problem ? "bad" : "pick" });
    if (!s.problem) {
      c.append(el("div", "radio"));
      c.onclick = () => {
        list.querySelectorAll(".card").forEach((x) => x.classList.remove("sel"));
        c.classList.add("sel"); S.set = s; $("pick-next").disabled = false;
      };
    }
    list.append(c);
  });
  const ok = sets.filter((s) => !s.problem);
  if (ok.length === 1) list.querySelector(".card.pick").click();
}
$("go-join").onclick = () => { S.mode = "join"; loadScan(); };
$("pick-folder").onclick = async () => { const p = await invoke("pick_folder"); if (p) loadScan(p); };
$("pick-next").onclick = () => renderReady();

// ---------- דחיסה: בחירה ----------
$("go-compress").onclick = () => { S.mode = "compress"; S.file = null; renderCFile(); show("s-cpick"); };
function renderCFile() {
  const box = $("cfile"); box.textContent = "";
  $("cpick-next").disabled = !S.file;
  if (!S.file) { box.append(card({ icon: "?", value: "עדיין לא נבחר קובץ" })); return; }
  box.append(card({ icon: "▤", value: S.file.name, ltrValue: true, sub: human(S.file.size), cls: "sel" }));
}
$("cpick-file").onclick = async () => { const f = await invoke("pick_file"); if (f) { S.file = f; renderCFile(); } };
$("cpick-next").onclick = () => renderReady();

// ---------- מוכנים ----------
function renderReady() {
  const box = $("ready-cards"); box.textContent = "";
  const alert = $("ready-alert"); alert.hidden = true; alert.className = "alert";
  let canStart = true, free, need, outDir;
  S.overwrite = false;
  if (S.mode === "join") {
    const s = S.set; free = S.scan.free; outDir = S.scan.outDir; need = s.total + (64 << 20);
    $("ready-title").textContent = "מוכנים להרכיב";
    $("ready-hint").textContent = "בדוק שהכל נראה כמו שצריך ולחץ על התחל.";
    box.append(card({ icon: "▤", label: "ייווצר הקובץ", value: s.archive, ltrValue: true }));
    box.append(card({ icon: "⧉", label: "חלקים", value: `${s.parts.length} חלקים · ${human(s.total)}` }));
    box.append(card({ icon: "✓", label: "אימות", value: s.hasManifest ? "SHA-256 מול המניפסט" : "אין מניפסט — לא ניתן לאמת" }));
    S.overwrite = s.existsInOut;
    if (s.existsInOut) note(alert, `הקובץ ${s.archive} כבר קיים בתיקייה ויוחלף.`, true);
    else if (!s.hasManifest) note(alert, "לא נמצא קובץ מניפסט, ההרכבה תתבצע בלי אימות. מומלץ לשים אותו ליד החלקים.", true);
    $("del-wrap").hidden = !s.hasManifest; $("del-parts").checked = false;
    $("start").textContent = "התחל הרכבה";
  } else {
    const f = S.file; free = f.free; outDir = f.outDir; need = f.size + (64 << 20);
    $("ready-title").textContent = "מוכנים לדחוס";
    $("ready-hint").textContent = "הדחיסה זהה לבניית הספרייה: zstd ברמה 19.";
    box.append(card({ icon: "▤", label: "קובץ מקור", value: f.name, ltrValue: true, sub: human(f.size) }));
    box.append(card({ icon: "⧉", label: "ייווצר", value: f.name + ".zst", ltrValue: true }));
    box.append(card({ icon: "⚙", label: "דחיסה", value: "zstd רמה 19 · חלון 8MB · סכום ביקורת" }));
    S.overwrite = f.existsInOut;
    if (f.existsInOut) note(alert, `הקובץ ${f.name}.zst כבר קיים בתיקייה ויוחלף.`, true);
    $("del-wrap").hidden = true;
    $("start").textContent = "התחל דחיסה";
  }
  box.append(card({ icon: "📁", label: "תיקיית הפלט (היחידה שנכתבת)", value: outDir, ltrValue: true,
    sub: free != null ? `מקום פנוי: ${human(free)}` : null }));
  if (free != null && free < need) {
    note(alert, `אין מספיק מקום פנוי בתיקייה: נדרשים ${human(need)} ופנויים ${human(free)}.`, false);
    canStart = false;
  }
  $("start").disabled = !canStart;
  show("s-ready");
}
function note(a, text, warn) { a.textContent = text; a.hidden = false; a.className = "alert" + (warn ? " warn" : ""); }
$("ready-back").onclick = () => show(S.mode === "join" ? "s-pick" : "s-cpick");

// ---------- ריצה ----------
$("start").onclick = async () => {
  $("pct").textContent = "0%"; $("bar-fill").style.width = "0%";
  $("prog-meta").textContent = ""; $("prog-size").textContent = "";
  $("stop").disabled = false;
  if (S.mode === "join") {
    $("prog-title").textContent = "מרכיב את הספרייה…"; $("prog-sub").textContent = "אפשר לעצור בכל רגע, לא יישאר קובץ חלקי.";
  } else {
    $("prog-title").textContent = "דוחס את הקובץ…"; $("prog-sub").textContent = "רמה 19 איטית בכוונה, כדי להגיע לאותו גודל כמו הספרייה.";
  }
  show("s-progress");
  try {
    if (S.mode === "join") await invoke("start_join", { dir: S.set.dir, archive: S.set.archive, overwrite: S.overwrite });
    else await invoke("start_compress", { path: S.file.path, overwrite: S.overwrite });
  } catch (e) { fail(String(e)); }
};
$("stop").onclick = () => { $("stop").disabled = true; invoke("cancel"); };

listen("progress", (ev) => {
  const p = ev.payload; const pct = p.total ? (p.done / p.total) * 100 : 0;
  $("pct").textContent = `${Math.floor(pct)}%`; $("bar-fill").style.width = `${pct}%`;
  if (p.phase === "join") $("prog-meta").textContent = `מחבר חלק ${p.partIndex + 1} מתוך ${p.partCount}`;
  else $("prog-meta").textContent = "דוחס…";
  const eta = p.speed > 0 ? (p.total - p.done) / p.speed : 0;
  $("prog-size").textContent = `${human(p.done)} מתוך ${human(p.total)} · ${human(p.speed)}/ש׳ · נותרו כ-${dur(eta)}`;
});
listen("finished", async (ev) => {
  const d = (S.done = ev.payload);
  const box = $("done-cards"); box.textContent = "";
  box.append(card({ icon: "▤", label: "קובץ", value: d.name, ltrValue: true }));
  if (d.kind === "join") {
    box.append(card({ icon: "⧉", label: "גודל", value: human(d.size) }));
    box.append(card({ icon: "✓", label: "אימות", value: d.verified ? "אומת מול המניפסט ✓" : "לא אומת (אין מניפסט)", sub: d.sha256 ? "SHA-256: " + d.sha256.slice(0, 16) + "…" : null }));
  } else {
    box.append(card({ icon: "⧉", label: "גודל", value: `${human(d.inputSize)} ← ${human(d.size)}` }));
  }
  box.append(card({ icon: "⏱", label: "משך הפעולה", value: dur(d.seconds) }));
  $("del-now").hidden = !(d.partsDeletable && S.mode === "join");
  $("del-now").textContent = "מחק את החלקים (נשמר רק הקובץ המורכב)"; $("del-now").disabled = false; $("del-now").dataset.armed = "";
  show("s-done");
  if (S.mode === "join" && d.partsDeletable && $("del-parts").checked) await deleteParts();
});
listen("failed", (ev) => {
  const { cancelled, message } = ev.payload;
  $("err-badge").className = "badge " + (cancelled ? "warn" : "bad");
  $("err-title").textContent = cancelled ? "הפעולה נעצרה" : "משהו השתבש";
  $("err-msg").textContent = message; show("s-error");
});
function fail(msg) { $("err-badge").className = "badge bad"; $("err-title").textContent = "משהו השתבש"; $("err-msg").textContent = msg; show("s-error"); }

async function deleteParts() {
  const b = $("del-now");
  try { const n = await invoke("delete_parts", { dir: S.set.dir, archive: S.set.archive }); b.textContent = `נמחקו ${n} קבצים`; }
  catch (e) { b.textContent = String(e); }
  b.disabled = true;
}
$("del-now").onclick = () => {
  const b = $("del-now");
  if (b.dataset.armed) return deleteParts();
  b.dataset.armed = "1"; b.textContent = "לחץ שוב לאישור המחיקה";
};
$("reveal").onclick = () => S.done && invoke("reveal", { path: S.done.output });
$("finish").onclick = () => win.close();
$("retry").onclick = () => ((S.mode === "join" ? S.set : S.file) ? renderReady() : show("s-welcome"));
$("err-close").onclick = () => show("s-welcome");
