// Oberfläche von Breadbin (C64 Game Library). Spricht über Tauri-Befehle mit dem Rust-Teil.
"use strict";

const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;

const LIMIT = 1000;
const $ = (sel) => document.querySelector(sel);

let selectedSha1 = null;
// Eigene Listen: früh anlegen, die Trefferliste kann vor loadLists() gezeichnet werden
let gameLists = [];
let scanning = false;
// Quelle, die gerade gescannt wird, mit dem letzten Zwischenstand (für die Quellenliste).
let scanLive = null;
let scraping = false;
let lastRows = [];
// Fassungen des Spiels, dessen Details gerade zu sehen sind (beim Zusammenfassen).
let currentGroup = [];
let lastVariants = [];
// Mit Cmd/Strg- oder Shift-Klick markierte Zeilen (sha1 der Zeile); ab zwei gilt die Sammelansicht.
const marked = new Set();
let markAnchor = null;
let devices = [];

// Ergebnis einer Rückfrage mit eigenen Knöpfen: Tauri liefert den Text des gedrückten Knopfs
// (ältere Fassungen „Ok“ bzw. {Custom: Text}).
function confirmed(answer, label) {
  return answer === "Ok" || answer === label || answer?.Custom === label;
}

function esc(s) {
  return String(s ?? "").replace(/[&<>"']/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" })[c]);
}

function fmtDate(secs) {
  return secs ? new Date(secs * 1000).toLocaleString(LOCALE, { dateStyle: "short", timeStyle: "short" }) : t("noch nie");
}

function fmtSize(bytes) {
  const n = (v) => v.toLocaleString(LOCALE, { maximumFractionDigits: 1 });
  if (bytes >= 1024 ** 3) return `${n(bytes / 1024 ** 3)} GB`;
  if (bytes >= 10 * 1024 ** 2) return `${n(bytes / 1024 ** 2)} MB`;
  return bytes >= 1024 ? `${Math.round(bytes / 1024)} KB` : `${bytes} B`;
}

function setStatus(text) {
  $("#status-text").textContent = text;
  $("#btn-errors").hidden = true;
}

// Fehlerliste des letzten Scans/Abrufs hinter einem Knopf in der Statusleiste.
let lastErrors = { title: "", list: [] };
function showErrorsButton(title, list) {
  lastErrors = { title, list };
  $("#btn-errors").textContent = t("{n} Fehler anzeigen", { n: list.length });
  $("#btn-errors").hidden = false;
}
$("#btn-errors").addEventListener("click", () => {
  $("#info-title").textContent = lastErrors.title;
  $("#info-text").textContent = lastErrors.list.join("\n");
  $("#info-dialog").showModal();
});

async function showError(e) {
  setStatus(t("Fehler: {e}", { e }));
  try {
    await invoke("plugin:dialog|message", { message: String(e), title: t("Fehler"), kind: "error" });
  } catch {
    alert(e);
  }
}

// ---------------------------------------------------------------- Quellen

async function loadSources() {
  const sources = await invoke("list_sources");
  const ul = $("#sources");
  if (!sources.length) {
    ul.innerHTML = `<li class="empty">${t("Noch keine Quellen. Füge einen Ordner mit Spielen hinzu – auch ein eingebundenes Netzlaufwerk.")}</li>`;
  } else {
    // Während eines Scans sind die Knöpfe gesperrt; der Hinweis sagt beim Darüberfahren, warum.
    const locked = scanning ? `disabled title="${t("Während eines Scans gesperrt – unten in der Statusleiste lässt er sich abbrechen.")}"` : "";
    ul.innerHTML = sources.map((s) => {
      const live = scanLive?.id === s.id;
      const quickTitle = s.quick
        ? t("Ausschalten: Der nächste Scan lädt übernommene Dateien doch noch herunter und prüft sie.")
        : t("Einschalten: Dateien mit gleichem Namen und gleicher Größe wie in anderen Quellen werden übernommen statt heruntergeladen.");
      return `
      <li data-id="${s.id}" class="row ${live ? "scanning" : ""}">
        <div class="row-main" title="${esc(s.location)}">
          <div class="label">${esc(s.label)}${s.kind === "ultimate" ? `<span class="kind">${t("Gerät")}</span>` : ""}${s.quick ? `<span class="kind quiet" title="${t("Schnellscan an")}">${t("Schnell")}</span>` : ""}</div>
          <div class="meta" data-live>${live ? esc(scanLive.text)
            : t("{n} Dateien · Scan: {date}", { n: nf(s.files), date: fmtDate(s.last_scan) })}</div>
        </div>
        <button data-act="scan" class="small icon" ${locked || `title="${t("Scannen")}"`} aria-label="${t("Scannen")}">↻</button>
        <details class="row-menu">
          <summary title="${t("Weitere Aktionen")}" aria-label="${t("Weitere Aktionen")}">⋯</summary>
          <div class="menu">
            <div class="meta menu-info">${esc(s.location)}</div>
            ${s.kind === "ultimate" ? `<button data-act="quick" data-quick="${s.quick ? 1 : 0}"
              ${locked || `title="${quickTitle}"`}>${s.quick ? t("Schnellscan ausschalten") : t("Schnellscan einschalten")}</button>` : ""}
            <button data-act="remove" class="danger" ${locked}>${t("Entfernen")}</button>
          </div>
        </details>
      </li>`;
    }).join("");
  }
  $("#btn-scan-all").disabled = scanning || !sources.length;
  $("#sources-count").textContent = sources.length ? nf(sources.length) : "";
  const filter = $("#source-filter");
  const current = filter.value;
  filter.innerHTML = `<option value="">${t("Alle Quellen")}</option>` +
    sources.map((s) => `<option value="${s.id}">${esc(s.label)}</option>`).join("");
  filter.value = sources.some((s) => String(s.id) === current) ? current : "";
  await loadStats();
}

async function loadStats() {
  const s = await invoke("stats");
  $("#stats-count").textContent = nf(s.files);
  invoke("share_count").then((n) => {
    $("#share-count").textContent = n ? nf(n) : "";
  }).catch(() => {});
  // Beschreibungsstand für die Karte oben in der Seitenleiste.
  const open = Math.max(0, s.contents - s.with_meta - s.not_found);
  $("#ss-bar").style.width = s.contents ? `${((s.with_meta + s.not_found) / s.contents) * 100}%` : "0";
  $("#ss-progress-text").textContent = s.contents
    ? t("{ok} mit Beschreibung · {nf} nicht gefunden · {open} offen", { ok: nf(s.with_meta), nf: nf(s.not_found), open: nf(open) })
    : "";
  $("#stats").innerHTML = `
    <table>
      <tr><td>${t("Dateien")}</td><td>${nf(s.files)}</td></tr>
      <tr><td>${t("Verschiedene Inhalte")}</td><td>${nf(s.contents)}</td></tr>
      ${s.by_format.map(([f, n]) => `<tr><td>${esc(f)}</td><td>${nf(n)}</td></tr>`).join("")}
    </table>`;
}

$("#btn-add").addEventListener("click", async () => {
  try {
    const path = await invoke("plugin:dialog|open", { options: { directory: true, multiple: false, title: t("Ordner mit C64-Spielen wählen") } });
    if (!path) return;
    const id = await invoke("add_folder", { path });
    await loadSources();
    await startScan([id]);
  } catch (e) {
    showError(e);
  }
});

$("#sources").addEventListener("click", async (ev) => {
  const btn = ev.target.closest("button[data-act]");
  if (!btn) return;
  const id = Number(btn.closest("li").dataset.id);
  btn.closest("details")?.removeAttribute("open");
  try {
    if (btn.dataset.act === "scan") {
      await startScan([id]);
    } else if (btn.dataset.act === "quick") {
      await invoke("set_source_quick", { id, quick: btn.dataset.quick !== "1" });
      await loadSources();
    } else if (btn.dataset.act === "remove") {
      const label = btn.closest("li").querySelector(".label").textContent;
      const answer = await invoke("plugin:dialog|message", {
        message: t("„{label}“ aus der Datenbank entfernen? Die Dateien selbst bleiben unverändert.", { label }),
        title: t("Quelle entfernen"), kind: "warning",
        buttons: { OkCancelCustom: [t("Entfernen"), t("Abbrechen")] },
      });
      if (!confirmed(answer, t("Entfernen"))) return;
      await invoke("remove_source", { id });
      await loadSources();
      await runSearch();
    }
  } catch (e) {
    showError(e);
  }
});

$("#btn-scan-all").addEventListener("click", () => startScan([]).catch(showError));
$("#btn-cancel").addEventListener("click", () => invoke(scraping ? "cancel_scrape" : "cancel_scan"));

// ---------------------------------------------------------------- Scan

async function startScan(ids) {
  await invoke("start_scan", { ids });
  scanning = true;
  $("#btn-cancel").hidden = false;
  setStatus(t("Scan startet …"));
  await loadSources();
}

listen("scan-progress", async ({ payload: ev }) => {
  const p = ev.progress;
  if (ev.error) {
    showError(`${ev.source_label}: ${ev.error}`);
    return;
  }
  if (!ev.finished) {
    const quick = p.quick ? t(", {n} übernommen", { n: nf(p.quick) }) : "";
    setStatus(t("Scanne {source}: {read} neu{quick}, {unchanged} unverändert – {current}",
      { source: ev.source_label, read: nf(p.read), quick, unchanged: nf(p.unchanged), current: p.current }));
    showScanLive(ev.source_id, t("Wird gescannt … {read} neu{quick}, {unchanged} unverändert",
      { read: nf(p.read), quick, unchanged: nf(p.unchanged) }));
    return;
  }
  scanLive = null;
  const parts = [t("{n} neu gelesen", { n: nf(p.read) }), t("{n} unverändert", { n: nf(p.unchanged) })];
  if (p.quick) parts.push(t("{n} per Schnellscan übernommen", { n: nf(p.quick) }));
  if (p.removed) parts.push(t("{n} entfernt", { n: nf(p.removed) }));
  if (p.errors.length) parts.push(t("{n} Fehler", { n: p.errors.length }));
  setStatus(t("{source}: {end} – {parts}",
    { source: ev.source_label, end: p.cancelled ? t("abgebrochen") : t("fertig"), parts: parts.join(", ") }));
  if (p.errors.length) showErrorsButton(t("Fehler beim Scannen von {source}", { source: ev.source_label }), p.errors);
  await loadSources();
  await runSearch();
});

listen("scan-error", ({ payload }) => showError(payload));

// Zwischenstand direkt an der Quelle zeigen, ohne die Liste neu aufzubauen.
function showScanLive(id, text) {
  scanLive = { id, text };
  const li = $(`#sources li[data-id="${id}"]`);
  if (!li) return;
  li.classList.add("scanning");
  li.querySelector("[data-live]").textContent = text;
}

listen("scan-finished", async () => {
  scanning = false;
  scanLive = null;
  $("#btn-cancel").hidden = true;
  await loadSources();
  await loadScraperSettings();
});

// ---------------------------------------------------------------- Seitenleiste: Abschnitte und Menüs

// Abschnitte ein- und ausklappen; Zustand je Abschnitt gemerkt.
document.querySelectorAll("#sidebar section.side").forEach((sec) => {
  const key = `fold.${sec.dataset.key}`;
  let stored = null;
  try { stored = localStorage.getItem(key); } catch { /* egal */ }
  if (stored !== null) sec.classList.toggle("collapsed", stored === "1");
  const btn = sec.querySelector("button.fold");
  btn.setAttribute("aria-expanded", String(!sec.classList.contains("collapsed")));
  btn.addEventListener("click", () => {
    const collapsed = sec.classList.toggle("collapsed");
    btn.setAttribute("aria-expanded", String(!collapsed));
    try { localStorage.setItem(key, collapsed ? "1" : "0"); } catch { /* egal */ }
  });
});

// Offenes ⋯-Menü schließen, sobald woanders geklickt wird; immer nur eins offen.
document.addEventListener("click", (ev) => {
  document.querySelectorAll("details.row-menu[open]").forEach((d) => {
    if (!d.contains(ev.target)) d.removeAttribute("open");
  });
});

// ---------------------------------------------------------------- Liste

let searchTimer = null;
let searchSeq = 0;

function searchOptions() {
  const sourceId = $("#source-filter").value;
  return {
    query: $("#search").value,
    format: $("#format").value || null,
    source_id: sourceId ? Number(sourceId) : null,
    only_meta: $("#only-meta").checked,
    only_duplicates: $("#only-dupes").checked,
    only_favorites: $("#view").value === "fav",
    recent: $("#view").value === "recent",
    shareable: $("#view").value === "shareable",
    list_id: $("#view").value.startsWith("list:") ? Number($("#view").value.slice(5)) : null,
    only_damaged: $("#view").value === "damaged",
    without_meta: $("#view").value === "nometa",
    sort: sortState.column,
    desc: sortState.desc,
    limit: LIMIT,
    group: $("#group").checked,
  };
}

$("#group").checked = (() => {
  try { return localStorage.getItem("group") !== "0"; } catch { return true; }
})();
$("#group").addEventListener("change", () => {
  try { localStorage.setItem("group", $("#group").checked ? "1" : "0"); } catch { /* egal */ }
  runSearch().catch(showError);
});

$("#btn-export").addEventListener("click", async () => {
  try {
    const path = await invoke("plugin:dialog|save", {
      options: { title: t("Liste exportieren"), defaultPath: "breadbin.csv", filters: [{ name: "CSV", extensions: ["csv"] }] },
    });
    if (!path) return;
    const n = await invoke("export_csv", { options: searchOptions(), path });
    setStatus(t("{n} Einträge nach {path} exportiert.", { n: nf(n), path }));
  } catch (e) {
    showError(e);
  }
});

/** Bereich „Teilen“ in der Seitenleiste aufklappen und zeigen. */
function showSharePanel() {
  const sec = document.querySelector('section.side[data-key="share"]');
  if (sec.classList.contains("collapsed")) sec.querySelector(".fold").click();
  sec.scrollIntoView({ behavior: "smooth", block: "nearest" });
  sec.classList.add("flash");
  setTimeout(() => sec.classList.remove("flash"), 1200);
}

// Teilen: eigene Angaben als Austauschdatei speichern bzw. die eines anderen einlesen.
$("#btn-share-export").addEventListener("click", async () => {
  try {
    const n = await invoke("share_count");
    if (!n) {
      await invoke("plugin:dialog|message", {
        message: t("Es gibt noch nichts zu teilen: Geteilt werden selbst geschriebene Angaben (Ansicht „Selbst geschrieben (teilbar)“). Bei Spielen von ScreenScraper geht das mit „Eigene Fassung schreiben“."),
        title: t("Teilen"), kind: "info",
      });
      return;
    }
    const day = new Date().toISOString().slice(0, 10);
    const path = await invoke("plugin:dialog|save", {
      options: { title: t("Austauschdatei speichern"), defaultPath: `breadbin-teilen-${day}.zip`, filters: [{ name: t("Breadbin-Austauschdatei"), extensions: ["zip"] }] },
    });
    if (!path) return;
    setStatus(t("Austauschdatei wird geschrieben …"));
    const count = await invoke("export_share", { path });
    setStatus(t("{n} Spiele in {path} gespeichert.", { n: nf(count), path }));
  } catch (e) {
    showError(e);
  }
});

// Ergebnis eines Einlesens als Meldung (Austauschdatei oder Gemeinschaftsdaten).
async function reportImport(r, title) {
  const lines = [
    t("{n} von {total} Spielen übernommen, davon {lib} in deiner Sammlung.", { n: nf(r.imported), total: nf(r.games), lib: nf(r.in_library) }),
  ];
  if (r.kept) lines.push(t("{n} übergangen: Dafür gibt es schon eigene Angaben (oder von ScreenScraper, wenn es die bevorzugte Quelle ist).", { n: nf(r.kept) }));
  if (r.errors.length) lines.push(t("{n} fehlerhaft:", { n: nf(r.errors.length) }), ...r.errors.slice(0, 10));
  setStatus(lines[0]);
  await invoke("plugin:dialog|message", { message: lines.join("\n"), title, kind: r.errors.length ? "warning" : "info" });
  await loadStats();
  await runSearch();
}

$("#btn-community-update").addEventListener("click", async () => {
  const btn = $("#btn-community-update");
  btn.disabled = true;
  try {
    setStatus(t("Gemeinschaftsdaten werden geladen …"));
    const r = await invoke("community_update");
    await reportImport(r, t("Gemeinschaftsdaten aktualisiert"));
  } catch (e) {
    showError(e);
  } finally {
    btn.disabled = false;
  }
});

$("#btn-community-submit").addEventListener("click", async () => {
  try {
    const n = await invoke("share_count");
    if (!n) {
      await invoke("plugin:dialog|message", {
        message: t("Es gibt noch nichts zu teilen: Geteilt werden selbst geschriebene Angaben (Ansicht „Selbst geschrieben (teilbar)“). Bei Spielen von ScreenScraper geht das mit „Eigene Fassung schreiben“."),
        title: t("Teilen"), kind: "info",
      });
      return;
    }
    submitCounts = { all: n, pending: await invoke("submit_pending_count") };
    $("#submit-all").checked = false;
    showSubmitInfo();
    $("#submit-result").textContent = "";
    $("#submit-send").hidden = false;
    $("#submit-send").disabled = !submitCounts.pending;
    $("#submit-close").textContent = t("Abbrechen");
    $("#submit-dialog").showModal();
  } catch (e) {
    showError(e);
  }
});
// Eingesandt wird nur, was noch nicht (oder in anderer Fassung) eingesandt wurde – außer mit dem Haken.
let submitCounts = { all: 0, pending: 0 };
function showSubmitInfo() {
  const all = $("#submit-all").checked;
  const n = all ? submitCounts.all : submitCounts.pending;
  $("#submit-info").textContent = n
    ? (all ? t("Alle {n} Spiele mit selbst geschriebenen Angaben werden eingesandt.", { n: nf(n) })
      : t("{n} neue oder geänderte Spiele werden eingesandt (von {all} mit selbst geschriebenen Angaben).", { n: nf(n), all: nf(submitCounts.all) }))
    : t("Nichts Neues: Alle {all} Spiele mit selbst geschriebenen Angaben sind schon in dieser Fassung eingesandt.", { all: nf(submitCounts.all) });
  $("#submit-send").disabled = !n;
}
$("#submit-all").addEventListener("change", showSubmitInfo);
$("#submit-close").addEventListener("click", () => $("#submit-dialog").close());
$("#submit-form").addEventListener("submit", async (ev) => {
  ev.preventDefault();
  const out = $("#submit-result");
  $("#submit-send").disabled = true;
  out.textContent = t("Wird eingesandt …");
  try {
    const r = await invoke("community_submit", { name: $("#submit-name").value, hinweis: $("#submit-hinweis").value, all: $("#submit-all").checked });
    if (r.ok) {
      out.textContent = r.teile > 1
        ? t("Eingegangen in {k} Beiträgen (Nr. {ids}, zusammen {n} Spiele). Danke! Nach der Prüfung erscheinen sie in den Gemeinschaftsdaten.", { k: r.teile, ids: r.ids.join(", "), n: nf(r.spiele) })
        : t("Eingegangen (Nr. {id}, {n} Spiele). Danke! Nach der Prüfung erscheint er in den Gemeinschaftsdaten.", { id: r.id, n: nf(r.spiele) });
      // Fertig: nur noch bestätigen.
      $("#submit-send").hidden = true;
      $("#submit-close").textContent = t("OK");
    } else {
      out.textContent = [r.fehler || t("Abgelehnt."), ...r.probleme.slice(0, 10)].join("\n");
      $("#submit-send").disabled = false;
    }
  } catch (e) {
    out.textContent = String(e);
    $("#submit-send").disabled = false;
  }
});

// Übersichtsseite (Statistik und Suche) liegt neben der Austauschdatei: Adresse ohne Dateinamen.
function communityWebUrl() {
  const u = scraperSettings?.community_url || "";
  return /^https?:\/\/.+\.zip$/i.test(u) && !/github\.com/i.test(u) ? u.replace(/[^/]+$/, "") : "";
}
$("#community-web").addEventListener("click", (ev) => {
  ev.preventDefault();
  const url = communityWebUrl();
  if (url) invoke("plugin:opener|open_url", { url }).catch(showError);
});

$("#btn-community-contribute").addEventListener("click", async () => {
  try {
    if (!(await invoke("share_count"))) {
      await invoke("plugin:dialog|message", {
        message: t("Es gibt noch nichts zu teilen: Geteilt werden selbst geschriebene Angaben (Ansicht „Selbst geschrieben (teilbar)“). Bei Spielen von ScreenScraper geht das mit „Eigene Fassung schreiben“."),
        title: t("Teilen"), kind: "info",
      });
      return;
    }
    const dir = await invoke("plugin:dialog|open", { options: { directory: true, multiple: false, title: t("Ordner für den Beitrag wählen (z. B. Arbeitskopie des Forks)") } });
    if (!dir) return;
    const n = await invoke("community_contribute", { dir });
    setStatus(t("{n} Spiele für den Beitrag nach {path} geschrieben.", { n: nf(n), path: dir }));
    await invoke("plugin:dialog|message", {
      message: t("{n} Spiele liegen jetzt in {path} (Ordner games/ und images/).\n\nNächste Schritte: Liegt der Ordner in deinem Fork der Gemeinschaftsdatenbank, die Änderungen committen und einen Pull Request stellen. Die Prüfung läuft dort automatisch.", { n: nf(n), path: dir }),
      title: t("Beitrag vorbereitet"), kind: "info",
    });
  } catch (e) {
    showError(e);
  }
});

$("#btn-share-import").addEventListener("click", async () => {
  try {
    const path = await invoke("plugin:dialog|open", {
      options: { title: t("Austauschdatei einlesen"), multiple: false, filters: [{ name: t("Breadbin-Austauschdatei"), extensions: ["zip"] }] },
    });
    if (!path) return;
    setStatus(t("Austauschdatei wird eingelesen …"));
    const r = await invoke("import_share", { path });
    await reportImport(r, t("Austauschdatei eingelesen"));
  } catch (e) {
    showError(e);
  }
});

async function runSearch() {
  const seq = ++searchSeq;
  const options = searchOptions();
  const rows = await invoke("search", { options });
  if (seq !== searchSeq) return; // eine neuere Suche läuft schon
  lastRows = rows;
  for (const sha1 of [...marked]) if (!rows.some((r) => r.sha1 === sha1)) marked.delete(sha1);
  const multi = marked.size > 1;

  $("#count").textContent = rows.length >= LIMIT
    ? t("mehr als {n} Treffer – Suche verfeinern", { n: nf(LIMIT) })
    : t("{n} Treffer", { n: nf(rows.length) });
  $("#list tbody").innerHTML = rows.map((r) => `
    <tr data-sha1="${r.sha1}" class="${(multi ? marked.has(r.sha1) : r.variants.includes(selectedSha1)) ? "selected" : ""}">
      <td class="star ${r.favorite ? "on" : ""}" data-star title="${t("Favorit")}">${r.favorite ? "★" : "☆"}</td>
      <td title="${esc(r.title)}">${esc(r.title)}${r.meta_status === "ok" ? `<span class="dot" title="${t("Beschreibung vorhanden")}"></span>` : ""}${r.done ? `<span class="done-mark" title="${t("Durchgespielt")}">✓</span>` : ""}${r.rating ? `<span class="rating-mark" title="${t("Eigene Bewertung: {n} von 5", { n: r.rating })}">${"★".repeat(r.rating)}</span>` : ""}${r.disks ? `<span class="disks" title="${t("Diskettenreihe: {n} Disks bzw. Seiten", { n: r.disks })}">💾${r.disks}</span>` : ""}</td>
      <td>${esc(r.year)}</td>
      <td title="${esc(r.publisher)}">${esc(r.publisher)}</td>
      <td title="${esc(tGenres(r.genres))}">${esc(tGenres(r.genres))}</td>
      <td title="${r.variants.length > 1 ? t("{n} Fassungen", { n: r.variants.length }) : ""}">${r.formats.split(", ").map((f) => `<span class="badge">${esc(f)}</span>`).join(" ")}${r.variants.length > 1 ? `<span class="variants">×${r.variants.length}</span>` : ""}</td>
      <td title="${esc(tDetail(r.detail))}">${esc(r.image_name || tDetail(r.detail))}</td>
      <td class="num">${r.locations}</td>
    </tr>`).join("");

  const empty = $("#empty");
  const hasSources = $("#sources li[data-id]") !== null;
  empty.hidden = rows.length > 0;
  empty.textContent = hasSources ? t("Keine Treffer.") : t("Füge links einen Ordner hinzu, um deine Spiele zu erfassen.");
}

$("#search").addEventListener("input", () => {
  clearTimeout(searchTimer);
  searchTimer = setTimeout(() => runSearch().catch(showError), 200);
});
$("#format").addEventListener("change", () => runSearch().catch(showError));
["#source-filter", "#only-meta", "#only-dupes"].forEach((sel) =>
  $(sel).addEventListener("change", () => runSearch().catch(showError)));
// Ansicht; der letzte Eintrag „Listen verwalten …“ öffnet den Dialog und lässt die Ansicht, wie sie war.
let viewBefore = "";
$("#view").addEventListener("change", (ev) => {
  if (ev.target.value === "manage-lists") {
    ev.target.value = viewBefore;
    openListsDialog().catch(showError);
    return;
  }
  viewBefore = ev.target.value;
  runSearch().catch(showError);
});

// Sortierung per Klick auf die Spaltenköpfe; zweiter Klick dreht die Richtung um.
const sortState = (() => {
  try { return JSON.parse(localStorage.getItem("sort")) ?? { column: "title", desc: false }; }
  catch { return { column: "title", desc: false }; }
})();

function showSort() {
  document.querySelectorAll("#list th[data-sort]").forEach((th) => {
    th.classList.toggle("sorted", th.dataset.sort === sortState.column);
    th.classList.toggle("desc", th.dataset.sort === sortState.column && sortState.desc);
  });
}

document.querySelectorAll("#list th[data-sort]").forEach((th) => th.addEventListener("click", () => {
  if (sortState.column === th.dataset.sort) {
    sortState.desc = !sortState.desc;
  } else {
    sortState.column = th.dataset.sort;
    sortState.desc = th.dataset.sort === "locations"; // Orte: die häufigsten zuerst
  }
  try { localStorage.setItem("sort", JSON.stringify(sortState)); } catch { /* egal */ }
  showSort();
  runSearch().catch(showError);
}));
showSort();

async function toggleFavorite(sha1) {
  const on = await invoke("toggle_favorite", { sha1 });
  const row = lastRows.find((r) => r.sha1 === sha1);
  if (row) row.favorite = on;
  document.querySelectorAll(`tr[data-sha1="${sha1}"] td[data-star], #details .star-btn[data-sha1="${sha1}"]`).forEach((el) => {
    el.classList.toggle("on", on);
    el.textContent = on ? "★" : "☆";
  });
  if ($("#view").value === "fav") runSearch().catch(showError);
}

$("#list tbody").addEventListener("click", (ev) => {
  const tr = ev.target.closest("tr[data-sha1]");
  if (!tr) return;
  if (ev.target.closest("td[data-star]")) {
    toggleFavorite(tr.dataset.sha1).catch(showError);
    return;
  }
  const sha1 = tr.dataset.sha1;
  if (ev.metaKey || ev.ctrlKey || (ev.shiftKey && markAnchor)) {
    // Bisher einzeln gewählte Zeile zählt als erste Markierung.
    const cur = $("#list tr.selected");
    if (!marked.size && cur) marked.add(cur.dataset.sha1);
    if (ev.shiftKey && markAnchor) {
      window.getSelection()?.removeAllRanges();
      const idx = (x) => lastRows.findIndex((r) => r.sha1 === x);
      const [a, b] = [idx(markAnchor), idx(sha1)].sort((x, y) => x - y);
      if (a >= 0) lastRows.slice(a, b + 1).forEach((r) => marked.add(r.sha1));
    } else if (marked.has(sha1)) {
      marked.delete(sha1);
    } else {
      marked.add(sha1);
    }
    markAnchor = sha1;
    document.querySelectorAll("#list tbody tr").forEach((r) => r.classList.toggle("selected", marked.has(r.dataset.sha1)));
    if (marked.size > 1) {
      showMarked();
      return;
    }
    const only = [...marked][0];
    marked.clear();
    if (only) showDetails(only).catch(showError);
    return;
  }
  marked.clear();
  markAnchor = sha1;
  document.querySelectorAll("#list tr.selected").forEach((r) => r.classList.remove("selected"));
  tr.classList.add("selected");
  currentGroup = lastRows.find((r) => r.sha1 === sha1)?.variants ?? [sha1];
  showDetails(sha1).catch(showError);
});

// Zufallsspiel: eine Zeile der aktuellen Treffer wählen, markieren, in den sichtbaren Bereich holen und zeigen.
function randomGame() {
  if (!lastRows.length) return;
  const pick = lastRows.length === 1 ? lastRows[0]
    : (() => { let r; do { r = lastRows[Math.floor(Math.random() * lastRows.length)]; } while (r.variants.includes(selectedSha1)); return r; })();
  marked.clear();
  markAnchor = pick.sha1;
  document.querySelectorAll("#list tr.selected").forEach((r) => r.classList.remove("selected"));
  const tr = document.querySelector(`#list tr[data-sha1="${pick.sha1}"]`);
  tr?.classList.add("selected");
  tr?.scrollIntoView({ block: "center" });
  currentGroup = pick.variants;
  showDetails(pick.sha1).catch(showError);
}
$("#btn-random").addEventListener("click", randomGame);
document.addEventListener("keydown", (ev) => {
  if (ev.key !== "z" && ev.key !== "Z") return;
  if (ev.metaKey || ev.ctrlKey || ev.altKey || ev.target.closest("input, textarea, select, [contenteditable]") || document.querySelector("dialog[open]")) return;
  randomGame();
});

// Sammelansicht für mehrere markierte Zeilen.
function showMarked() {
  const rows = lastRows.filter((r) => marked.has(r.sha1));
  const sha1s = rows.flatMap((r) => r.variants);
  const withMeta = rows.filter((r) => r.meta_status === "ok").flatMap((r) => r.variants);
  const open = rows.filter((r) => r.meta_status !== "ok").flatMap((r) => r.variants);
  const pane = $("#details");
  pane.hidden = false;
  pane.innerHTML = `
    <h3>${t("{n} Einträge markiert", { n: rows.length })}</h3>
    <div class="sub">${sha1s.length > rows.length ? t("{n} Fassungen insgesamt", { n: sha1s.length }) : "&nbsp;"}</div>
    <div class="meta-actions marked-actions">
      <button class="primary" data-act="pick-all">${t("Gemeinsam einem Spiel zuordnen")}</button>
      <button data-act="own-all">${t("Angaben selbst eintragen")}</button>
      ${open.length ? `<button data-act="lookup-all">${t("Bei ScreenScraper nachschlagen ({n} ohne Beschreibung)", { n: open.length })}</button>` : ""}
      ${withMeta.length ? `<button data-act="reject-all">${t("Falsches Spiel ({n})", { n: withMeta.length })}</button>` : ""}
      ${withMeta.length ? `<button data-act="clear-all" class="danger">${t("Angaben löschen ({n})", { n: withMeta.length })}</button>` : ""}
      <button class="link" data-act="unmark">${t("Markierung aufheben")}</button>
    </div>
    <div class="meta-actions marked-actions">
      <select data-act="list-pick">${gameLists.map((l) => `<option value="${l.id}">${esc(l.name)}</option>`).join("")}<option value="new">${t("Neue Liste …")}</option></select>
      <input data-act="list-new" placeholder="${t("Name der neuen Liste")}" ${gameLists.length ? "hidden" : ""}>
      <button data-act="list-add">${t("In Liste aufnehmen")}</button>
      ${$("#view").value.startsWith("list:") ? `<button data-act="list-remove">${t("Aus dieser Liste entfernen")}</button>` : ""}
      ${devices.length ? `<button data-act="copy-device">${t("Auf Gerät kopieren …")}</button>` : ""}
    </div>
    <p class="meta">${t("Cmd- bzw. Strg-Klick markiert einzelne Zeilen, Shift-Klick einen Bereich.")}</p>
    <ul class="marked-list">${rows.map((r) => `<li>${esc(r.title)} <span class="meta">${esc(r.formats)}${r.variants.length > 1 ? ` ×${r.variants.length}` : ""}</span></li>`).join("")}</ul>`;
  pane.querySelector("[data-act=own-all]").addEventListener("click", () =>
    openOwnEditor(sha1s, null, (rows.find((r) => r.meta_status === "ok") ?? rows[0]).title));
  pane.querySelector("[data-act=pick-all]").addEventListener("click", () =>
    openGamePicker(rows.find((r) => r.meta_status === "ok") ?? rows[0], sha1s, true));
  pane.querySelector("[data-act=lookup-all]")?.addEventListener("click", () =>
    startScrape({ sha1s: open, retryNotfound: true }).catch(showError));
  pane.querySelector("[data-act=reject-all]")?.addEventListener("click", () => rejectMeta(withMeta).catch(showError));
  pane.querySelector("[data-act=clear-all]")?.addEventListener("click", () => clearMeta(withMeta).catch(showError));
  const pickSel = pane.querySelector("[data-act=list-pick]");
  const newInput = pane.querySelector("[data-act=list-new]");
  pickSel.addEventListener("change", () => { newInput.hidden = pickSel.value !== "new"; if (!newInput.hidden) newInput.focus(); });
  pane.querySelector("[data-act=list-add]").addEventListener("click", async () => {
    try {
      const id = pickSel.value === "new" ? await invoke("create_list", { name: newInput.value }) : Number(pickSel.value);
      await invoke("set_in_list", { id, sha1s, on: true });
      await loadLists();
      setStatus(t("{n} Spiele in „{list}“ aufgenommen.", { n: rows.length, list: gameLists.find((l) => l.id === id)?.name ?? "" }));
    } catch (e) { showError(e); }
  });
  pane.querySelector("[data-act=list-remove]")?.addEventListener("click", async () => {
    try {
      await invoke("set_in_list", { id: Number($("#view").value.slice(5)), sha1s, on: false });
      marked.clear();
      await loadLists();
      await runSearch();
      pane.hidden = true;
    } catch (e) { showError(e); }
  });
  pane.querySelector("[data-act=copy-device]")?.addEventListener("click", () => openCopyDialog(rows).catch(showError));
  pane.querySelector("[data-act=unmark]").addEventListener("click", () => {
    marked.clear();
    document.querySelectorAll("#list tr.selected").forEach((r) => r.classList.remove("selected"));
    if (selectedSha1) showDetails(selectedSha1).catch(showError);
  });
}

// Pfeiltasten wechseln die Auswahl in der Liste.
document.addEventListener("keydown", (ev) => {
  if (ev.key === "Escape" && marked.size > 1 && !document.querySelector("dialog[open]")) {
    $("#details button[data-act=unmark]")?.click();
    return;
  }
  if (ev.key !== "ArrowDown" && ev.key !== "ArrowUp") return;
  const cur = $("#list tr.selected");
  const next = cur ? (ev.key === "ArrowDown" ? cur.nextElementSibling : cur.previousElementSibling)
                   : $("#list tbody tr");
  if (!next) return;
  ev.preventDefault();
  next.click();
  next.scrollIntoView({ block: "nearest" });
});

// ---------------------------------------------------------------- Details

// Suche nach dem Spiel auf anderen Seiten (nur Links, keine Daten übernommen).
function webLinks(r, m) {
  // Ohne Untertitel: „USS John Young : The Naval …“ → „USS John Young“.
  const name = searchTextFor(m?.status === "ok" && m.title ? m.title : r.title).split(/\s*:\s/)[0];
  const q = encodeURIComponent(name).replace(/%20/g, "+");
  // C64-Wiki: Die Suchseite weist Aufrufe von außen ab, Artikeladressen gehen. Fehlt der Artikel,
  // bietet das Wiki dort selbst die Suche nach dem Titel an.
  const wiki = `https://www.c64-wiki.${LANG === "de" ? "de" : "com"}/wiki/${encodeURIComponent(name.replace(/ /g, "_"))}`;
  const links = [
    m?.status === "ok" && m.game_id ? ["ScreenScraper", `https://www.screenscraper.fr/gameinfos.php?gameid=${encodeURIComponent(m.game_id)}&plateforme=66`] : null,
    ["CSDb", `https://csdb.dk/search/?seinsel=releases&search=${q}`],
    ["GameBase64", `https://gb64.com/search.php?f=0&t=2&s=${q}&d=24&h=0&a=0`],
    ["Lemon64", `https://www.lemon64.com/games/list.php?list_title=${q}`],
    ["C64-Wiki", wiki],
  ].filter(Boolean);
  return `<div class="web-links"><span class="meta">${t("Im Netz suchen:")}</span>
    ${links.map(([label, url]) => `<button class="pill" data-url="${esc(url)}" title="${esc(url)}">${esc(label)}</button>`).join("")}</div>`;
}

function c64Directory(d) {
  const r = d.row;
  if (!["D64", "D71", "D81", "T64"].includes(r.format)) return "";
  const head = `0 <span class="head">"${esc((r.image_name ?? "").padEnd(16))}" ${esc((r.detail ?? "").replace(/^ID /, ""))}</span>`;
  const lines = d.entries.map((e) =>
    `${String(e.blocks).padEnd(5)}${esc(`"${e.name}"`.padEnd(19))}${esc(e.kind)}`);
  return `<h4>${t("Verzeichnis")}</h4><div class="c64dir">${[head, ...lines].join("\n")}</div>`;
}

async function showDetails(sha1) {
  selectedSha1 = sha1;
  if (!currentGroup.includes(sha1)) currentGroup = lastRows.find((r) => r.variants.includes(sha1))?.variants ?? [sha1];
  const group = currentGroup;
  const [d, variants] = await Promise.all([
    invoke("details", { sha1 }),
    group.length > 1 ? invoke("game_variants", { sha1s: group }) : Promise.resolve([]),
  ]);
  if (selectedSha1 !== sha1) return; // inzwischen etwas anderes gewählt
  lastVariants = variants;
  const r = d.row;
  const info = [
    [t("Format"), r.format],
    [t("Größe"), fmtSize(r.size)],
    [t("Jahr"), r.year],
    [t("Herausgeber"), r.publisher],
    [r.format === "CRT" ? t("Modulname") : t("Name im Abbild"), r.format === "D64" || r.format === "D71" || r.format === "D81" ? null : r.image_name],
    [t("Technik"), tDetail(r.detail)],
  ].filter(([, v]) => v);

  const m = d.meta;
  if (m?.status === "ok") {
    info.splice(2, 0, ...[
      [t("Genre"), tGenres(r.genres)],
      [t("Entwickler"), m.developer],
      [t("Spieler"), m.players],
      [t("Bewertung"), m.rating != null ? t("{n} von 20", { n: m.rating }) : null],
    ].filter(([, v]) => v));
  }
  // Korrekturen gelten für alle Fassungen mit derselben Zuordnung, sonst nur für diese.
  const affected = r.game_id ? variants.filter((v) => v.game_id === r.game_id).map((v) => v.sha1) : [];
  if (!affected.includes(sha1)) affected.unshift(sha1);
  // Herkunft: selbst geschrieben (teilbar), auf Grundlage von ScreenScraper bearbeitet, Gemeinschaft, ScreenScraper.
  const written = m?.provider === "own";
  const edited = m?.provider === "own:edited";
  const own = written || edited;
  const shared = m?.provider === "shared";
  const fromSs = m?.status === "ok" && (m.provider.startsWith("screenscraper") || edited);
  const pick = `<button class="link" data-act="pick">${m?.status === "ok" ? t("Anderes Spiel wählen") : t("Spiel von Hand suchen")}</button>`
    + `<button class="link" data-act="own">${m?.status === "ok" ? t("Angaben bearbeiten") : t("Angaben selbst eintragen")}</button>`
    + (fromSs ? `<button class="link" data-act="own-fresh" title="${t("Mit leerem Formular beginnen – selbst Geschriebenes kann geteilt werden, von ScreenScraper Übernommenes nicht")}">${t("Eigene Fassung schreiben")}</button>` : "")
    + (written ? `<button class="link" data-act="share">${t("Mit anderen teilen")}</button>` : "")
    + (m?.status === "ok" && m.game_id && ssReady()
      ? `<button class="link" data-act="propose">${t("Bei ScreenScraper vorschlagen")}</button>` : "");
  let metaHtml;
  if (!m) {
    metaHtml = `<div class="meta-actions"><button data-act="lookup">${t("Bei ScreenScraper nachschlagen")}</button>${pick}</div>`;
  } else if (m.status === "notfound") {
    metaHtml = `<div class="meta-actions"><span class="meta">${t("Bei ScreenScraper nicht gefunden ({date}).", { date: fmtDate(m.fetched_at) })}</span>
      <button class="link" data-act="lookup">${t("Erneut versuchen")}</button>${pick}</div>`;
  } else if (m.status === "rejected") {
    metaHtml = `<div class="meta-actions"><span class="meta">${t("Zuordnung als falsch markiert ({date}).", { date: fmtDate(m.fetched_at) })}</span>
      ${pick}<button class="link" data-act="unreject">${t("Zurücknehmen")}</button>
      ${m.game_id ? `<button class="link" data-act="report">${t("Fehler bei ScreenScraper melden")}</button>` : ""}</div>`;
  } else {
    metaHtml = `
      ${d.media.length ? `<div class="shots">${d.media.map(([kind]) =>
        `<img data-kind="${esc(kind)}" class="${kind === "box-2D" ? "box" : ""}" alt="${esc(kind)}">`).join("")}</div>` : ""}
      ${synopsisHtml(m)}
      ${m.provider === "screenscraper:name" ? `<p class="warn">${t("Über den Namen zugeordnet – bitte prüfen, ob es das richtige Spiel ist.")}</p>` : ""}
      ${m.provider === "screenscraper:manual" ? `<p class="meta">${t("Von Hand zugeordnet.")}</p>` : ""}
      ${written ? `<p class="meta">${t("Selbst geschrieben – wird beim Teilen mitgegeben.")}</p>` : ""}
      ${edited ? `<p class="meta">${t("Auf Grundlage von ScreenScraper bearbeitet – wird nicht geteilt. Teilbar wird es mit „Eigene Fassung schreiben“.")}</p>` : ""}
      ${shared ? `<p class="meta">${t("Aus der Gemeinschaft (Austauschdatei) übernommen.")}</p>` : ""}
      <div class="meta-actions">${shared ? `<button class="link" data-act="lookup">${t("Bei ScreenScraper nachschlagen")}</button>` : ""}${own || shared ? "" : `<button class="link" data-act="reject">${t("Falsches Spiel")}</button>`}${pick}
        <button class="link danger" data-act="clear">${t("Angaben löschen")}</button></div>`;
  }

  const pane = $("#details");
  pane.hidden = false;
  pane.innerHTML = `
    <h3><button class="star-btn ${r.favorite ? "on" : ""}" data-sha1="${r.sha1}" title="${t("Favorit")}">${r.favorite ? "★" : "☆"}</button>${esc(r.title)}</h3>
    ${r.last_launched ? `<div class="meta">${t("Zuletzt gestartet: {date}", { date: fmtDate(r.last_launched) })}</div>` : ""}
    <div class="sub">${[r.year, r.publisher].filter(Boolean).map(esc).join(" · ") || "&nbsp;"}</div>
    ${metaHtml}
    ${launchHtml(r)}
    <section class="mine" id="mine"></section>
    ${webLinks(r, m)}
    <dl>${info.map(([k, v]) => `<dt>${esc(k)}</dt><dd>${esc(v)}</dd>`).join("")}</dl>
    ${r.ungrouped ? `<div class="meta-actions"><span class="meta">${t("Aus der Gruppe gelöst.")}</span>
      <button class="link" data-act="regroup">${t("Wieder zusammenfassen")}</button></div>` : ""}
    ${disksHtml(variants)}
    ${variants.length > 1 ? `<h4>${t("{n} Fassungen", { n: variants.length })}</h4>
      <p class="meta">${t("Gehört eine Fassung nicht dazu, mit „Lösen“ abtrennen – sie steht dann allein in der Liste.")}</p>
      <ul class="variants-list">${variants.map((v) => `
        <li><button class="${v.sha1 === sha1 ? "current" : ""}" data-variant="${v.sha1}">
          <span class="badge">${esc(v.format)}</span>
          <span class="v-title">${esc(v.title)}${v.image_name && v.image_name !== v.title ? ` <span class="meta">${t("„{name}“", { name: esc(v.image_name) })}</span>` : ""}</span>
          <span class="meta">${fmtSize(v.size)} · ${v.locations === 1 ? t("1 Ort") : t("{n} Orte", { n: v.locations })}</span>
        </button>${devices.length && DISK_FORMATS.includes(v.format)
          ? `<button class="small insert" data-insert="${v.sha1}" title="${t("In Laufwerk A einlegen, ohne Neustart – für den Diskwechsel im laufenden Spiel")}">${t("Einlegen")}</button>` : ""}<button class="small ungroup" data-ungroup="${v.sha1}" title="${t("Diese Fassung ist ein anderes Spiel: aus der Gruppe lösen und allein anzeigen")}">${t("Lösen")}</button></li>`).join("")}</ul>` : ""}
    ${c64Directory(d)}
    <h4>${d.locations.length === 1 ? t("Gespeichert an 1 Ort") : t("Gespeichert an {n} Orten", { n: d.locations.length })}</h4>
    <ul class="locations">
      ${d.locations.map((l, i) => `
        <li>
          <div class="src">${esc(l.source_label)}</div>
          <div>${esc(l.path)}${l.member ? ` → ${esc(l.member)}` : ""}</div>
          ${l.quick ? `<div class="meta" title="${t("Gleicher Name und gleiche Größe wie in einer anderen Quelle; der Inhalt wurde nicht heruntergeladen und geprüft.")}">${t("Per Schnellscan übernommen, nicht geprüft")}</div>` : ""}
          ${l.source_kind === "local" ? `<button class="link" data-loc="${i}">${t("Im Ordner zeigen")}</button>` : ""}
        </li>`).join("")}
    </ul>
    <h4>${t("Prüfsummen")}</h4>
    <dl><dt>CRC32</dt><dd>${esc(d.crc32)}</dd><dt>MD5</dt><dd>${esc(d.md5)}</dd><dt>SHA-1</dt><dd>${esc(r.sha1)}</dd></dl>
    ${m?.status === "ok" && !own && !shared ? `<div class="meta-actions"><button class="link" data-act="lookup">${t("Angaben neu laden")}</button></div>
      <p class="credit">${t("Beschreibung und Bilder: ScreenScraper.fr (Stand {date})", { date: fmtDate(m.fetched_at) })}</p>`
      : (own || shared) && d.media.some(([, f]) => f.startsWith("ss_"))
        ? `<p class="credit">${t("Bilder teilweise: ScreenScraper.fr – sie werden nicht geteilt.")}</p>` : ""}`;

  wireLaunch(pane, r);
  renderMine(pane.querySelector("#mine"), [sha1, ...group.filter((x) => x !== sha1)]).catch(showError);
  pane.querySelectorAll(".web-links button[data-url]").forEach((btn) =>
    btn.addEventListener("click", () => invoke("plugin:opener|open_url", { url: btn.dataset.url }).catch(showError)));
  pane.querySelectorAll("button[data-variant]").forEach((btn) =>
    btn.addEventListener("click", () => showDetails(btn.dataset.variant).catch(showError)));
  pane.querySelector(".star-btn").addEventListener("click", () => toggleFavorite(r.sha1).catch(showError));
  pane.querySelectorAll("button[data-act=lookup]").forEach((btn) =>
    btn.addEventListener("click", () => {
      // Von Hand zugeordnet: dasselbe Spiel neu laden statt neu suchen.
      const run = m?.provider === "screenscraper:manual" && m.game_id
        ? assignGame(affected, m.game_id)
        : startScrape({ sha1s: [sha1], force: true });
      run.catch(showError);
    }));
  pane.querySelectorAll("button[data-act=pick]").forEach((btn) =>
    btn.addEventListener("click", () => openGamePicker(r, affected)));
  // Eigene Angaben und Bilder: auf Wunsch für alle Fassungen der Zeile (auch ohne gemeinsame Spielnummer).
  const ownTargets = async () => {
    const all = [...new Set([sha1, ...affected, ...group])];
    return all.length > 1 ? await askOwnScope(sha1, all) : [sha1];
  };
  pane.querySelectorAll("button[data-act=own]").forEach((btn) =>
    btn.addEventListener("click", async () => {
      const targets = await ownTargets();
      if (targets) openOwnEditor(targets, d);
    }));
  pane.querySelectorAll("button[data-act=propose]").forEach((btn) =>
    btn.addEventListener("click", () => openPropose(d).catch(showError)));
  pane.querySelector("button[data-act=own-fresh]")?.addEventListener("click", async () => {
    const targets = await ownTargets();
    if (targets) openOwnEditor(targets, d, "", true);
  });
  pane.querySelector("button[data-act=share]")?.addEventListener("click", showSharePanel);
  pane.querySelector("button[data-act=reject]")?.addEventListener("click", () => rejectMeta(affected, sha1).catch(showError));
  pane.querySelector("button[data-act=clear]")?.addEventListener("click", () => clearMeta(affected, sha1).catch(showError));
  pane.querySelector("button[data-act=unreject]")?.addEventListener("click", async () => {
    try {
      await invoke("reject_metadata", { sha1s: [sha1], on: false });
      await refreshAfterChange();
    } catch (e) { showError(e); }
  });
  pane.querySelector("button[data-act=report]")?.addEventListener("click", () =>
    invoke("plugin:opener|open_url", { url: `https://www.screenscraper.fr/gameinfos.php?gameid=${encodeURIComponent(m.game_id)}&plateforme=66` }).catch(showError));
  pane.querySelector("button[data-act=regroup]")?.addEventListener("click", async () => {
    try {
      await invoke("set_ungrouped", { sha1, on: false });
      currentGroup = [];
      await refreshAfterChange();
    } catch (e) { showError(e); }
  });
  // „Lösen“ an einer Fassung: Sie steht danach allein; gezeigt bleibt die Gruppe ohne sie
  // (bzw. die gelöste Fassung, wenn sie gerade angezeigt war).
  pane.querySelectorAll("button[data-ungroup]").forEach((btn) => btn.addEventListener("click", async () => {
    const v = btn.dataset.ungroup;
    try {
      await invoke("set_ungrouped", { sha1: v, on: true });
      if (v === sha1) {
        currentGroup = [v];
      } else {
        currentGroup = currentGroup.filter((x) => x !== v);
      }
      await refreshAfterChange();
      setStatus(t("Fassung aus der Gruppe gelöst."));
    } catch (e) { showError(e); }
  }));
  d.media.forEach(([kind, file]) => {
    const img = pane.querySelector(`img[data-kind="${CSS.escape(kind)}"]`);
    invoke("media_data", { file }).then((url) => {
      if (selectedSha1 === sha1 && img) img.src = url;
    }).catch(() => img?.remove());
    img?.addEventListener("click", () => {
      $("#zoom img").src = img.src;
      $("#zoom img").style.imageRendering = kind === "box-2D" ? "auto" : "pixelated";
      $("#zoom").showModal();
    });
  });

  pane.querySelectorAll("button[data-loc]").forEach((btn) => {
    btn.addEventListener("click", async () => {
      const l = d.locations[Number(btn.dataset.loc)];
      try {
        const path = await invoke("local_path", { sourceId: l.source_id, path: l.path });
        await invoke("plugin:opener|reveal_item_in_dir", { paths: [path] });
      } catch (e) {
        showError(e);
      }
    });
  });
}

$("#zoom").addEventListener("click", () => $("#zoom").close());

// ---------------------------------------------------------------- Zuordnung korrigieren

async function refreshAfterChange() {
  await runSearch();
  if (marked.size > 1) showMarked();
  else if (selectedSha1) await showDetails(selectedSha1);
}

// Aus den Details einer Gruppe (current gesetzt): fragen, ob nur diese Fassung oder alle mit derselben
// Zuordnung gemeint sind. Gibt die gewählten Prüfsummen zurück, null bei Abbruch.
async function askScope(sha1s, current, message, title, action) {
  if (!current || sha1s.length < 2) {
    const answer = await invoke("plugin:dialog|message", {
      message, title, kind: "warning", buttons: { OkCancelCustom: [action, t("Abbrechen")] },
    });
    return confirmed(answer, action) ? sha1s : null;
  }
  const one = t("Nur diese Fassung");
  const all = t("Alle {n} Fassungen", { n: sha1s.length });
  const answer = await invoke("plugin:dialog|message", {
    message: message + "\n\n" + t("{n} Fassungen in dieser Gruppe haben dieselbe Zuordnung.", { n: sha1s.length }),
    title, kind: "warning", buttons: { YesNoCancelCustom: [one, all, t("Abbrechen")] },
  });
  if (answer === "Yes" || answer === one || answer?.Custom === one) return [current];
  if (answer === "No" || answer === all || answer?.Custom === all) return sha1s;
  return null;
}

// Eigene Angaben bearbeiten: für alle Fassungen der Zeile (Vorschlag) oder nur die gezeigte.
async function askOwnScope(current, sha1s) {
  const all = t("Alle {n} Fassungen", { n: sha1s.length });
  const one = t("Nur diese Fassung");
  const answer = await invoke("plugin:dialog|message", {
    message: t("Diese Zeile fasst {n} Fassungen zusammen. Angaben und Bilder für alle übernehmen oder nur für die gezeigte?", { n: sha1s.length }),
    title: t("Angaben bearbeiten"), kind: "info", buttons: { YesNoCancelCustom: [all, one, t("Abbrechen")] },
  });
  if (answer === "Yes" || answer === all || answer?.Custom === all) return sha1s;
  if (answer === "No" || answer === one || answer?.Custom === one) return [current];
  return null;
}

async function rejectMeta(sha1s, current = null) {
  const chosen = await askScope(sha1s, current,
    sha1s.length > 1 && !current
      ? t("Beschreibung und Bilder von {n} Fassungen verwerfen? Spätere Abrufe holen sie nicht zurück.", { n: sha1s.length })
      : t("Beschreibung und Bilder verwerfen? Spätere Abrufe holen sie nicht zurück."),
    t("Falsches Spiel"), t("Verwerfen"));
  if (!chosen) return;
  await invoke("reject_metadata", { sha1s: chosen, on: true });
  await refreshAfterChange();
}

// Angaben und Bilder löschen; der Eintrag gilt dann als nicht nachgeschlagen (anders als „Falsches Spiel“).
async function clearMeta(sha1s, current = null) {
  const chosen = await askScope(sha1s, current,
    (sha1s.length > 1 && !current
      ? t("Beschreibung und Bilder von {n} Fassungen löschen?", { n: sha1s.length })
      : t("Beschreibung und Bilder löschen?"))
      + "\n\n" + t("Die Einträge gelten danach als nicht nachgeschlagen; ein späterer Abruf kann sie neu holen. Damit das nicht passiert: „Falsches Spiel“."),
    t("Angaben löschen"), t("Löschen"));
  if (!chosen) return;
  await invoke("clear_meta", { sha1s: chosen });
  await refreshAfterChange();
}

async function assignGame(sha1s, gameId) {
  setStatus(t("Lade Angaben zu Spiel {id} …", { id: gameId }));
  const warning = await invoke("assign_game", { sha1s, gameId: String(gameId) });
  setStatus(t("Zugeordnet."));
  if (warning) showError(warning);
  // Die Fassung kann dabei in eine andere Gruppe wechseln: Gruppe neu bestimmen.
  currentGroup = [];
  // Nach dem gemeinsamen Zuordnen: das Spiel mit all seinen Fassungen zeigen.
  if (marked.size > 1) {
    marked.clear();
    selectedSha1 = sha1s[0];
    currentGroup = [];
  }
  await refreshAfterChange();
}

// Suchtext ohne Szene-Kennungen (wie clean_title im Rust-Teil, vereinfacht).
function searchTextFor(title) {
  return title.replace(/\s\+\d\w*/g, "").split(/\s+/)
    .filter((w) => !/^v\d[\w]*$/i.test(w) && !/^\d{6}(\d\d)?([-_].*)?$/.test(w)).join(" ").trim();
}

let pickTarget = null;

// Aus den Details: standardmäßig nur die gezeigte Fassung; die übrigen mit derselben Zuordnung
// nur auf Wunsch (sonst landet ein einzelner Fehltreffer mit der ganzen Gruppe beim neuen Spiel).
function openGamePicker(row, sha1s, fromMarks = false) {
  const others = fromMarks ? [] : sha1s.filter((s) => s !== row.sha1);
  pickTarget = () => fromMarks ? sha1s : $("#pick-all")?.checked ? [row.sha1, ...others] : [row.sha1];
  $("#pick-query").value = searchTextFor(row.title);
  const count = $("#pick-count");
  if (fromMarks && sha1s.length > 1) count.textContent = t("Gilt für alle {n} markierten Fassungen.", { n: sha1s.length });
  else if (others.length) {
    count.innerHTML = `<label class="inline"><input type="checkbox" id="pick-all"> ${esc(others.length === 1
      ? t("Auch für die andere Fassung mit derselben Zuordnung")
      : t("Auch für die anderen {n} Fassungen mit derselben Zuordnung", { n: others.length }))}</label>`;
  } else count.textContent = "";
  $("#pick-list").innerHTML = "";
  $("#pick-dialog").showModal();
  runGameSearch().catch(showError);
}

// Herkunft eines Treffers aus der eigenen Bibliothek, für die Anzeige.
function providerLabel(p) {
  if (p === "own") return t("selbst geschrieben");
  if (p === "own:edited") return t("selbst bearbeitet");
  if (p === "shared") return t("Gemeinschaft");
  return "ScreenScraper";
}

// Suche im Zuordnen-Dialog: erst die eigene Bibliothek (eigene Angaben, Gemeinschaft, schon
// zugeordnete Spiele), dann ScreenScraper. Eine reine Zahl ist die Spielnummer bei ScreenScraper.
async function runGameSearch() {
  const query = $("#pick-query").value.trim();
  const list = $("#pick-list");
  if (!query) return;
  const target = pickTarget();
  let local = [];
  if (!/^\d+$/.test(query)) {
    // Die markierten Fassungen selbst gehören dazu: Hat die D64 Angaben und die PRG nicht, ist die
    // D64 genau der Eintrag, dessen Angaben alle übernehmen sollen.
    try { local = await invoke("search_library", { query, exclude: [] }); } catch { local = []; }
  }
  const localHtml = local.length ? `<li class="pick-head">${t("In deiner Bibliothek")}</li>` + local.map((h, i) => `
    <li><button type="button" data-local="${i}">
      <strong>${esc(h.title)}</strong>
      <span class="meta">${[h.year, h.publisher, tGenres(h.genres), providerLabel(h.provider),
        h.versions > 1 ? t("{n} Fassungen", { n: h.versions }) : ""].filter(Boolean).map(esc).join(" · ")}</span>
      ${h.synopsis ? `<span class="hit-text">${esc(h.synopsis)}</span>` : ""}
    </button></li>`).join("") : "";
  const wire = () => list.querySelectorAll("button[data-local]").forEach((btn) => btn.addEventListener("click", async () => {
    const hit = local[Number(btn.dataset.local)];
    $("#pick-dialog").close();
    try { await copyMeta(hit, target); } catch (e) { showError(e); }
  }));
  list.innerHTML = localHtml + `<li class="pick-head">ScreenScraper</li><li class="meta">${t("Suche bei ScreenScraper …")}</li>`;
  wire();
  let hits;
  try {
    hits = await invoke("search_games", { query });
  } catch (e) {
    list.innerHTML = localHtml + `<li class="pick-head">ScreenScraper</li><li class="meta">${esc(String(e))}</li>`;
    wire();
    return;
  }
  if (!hits.length) {
    list.innerHTML = localHtml + `<li class="pick-head">ScreenScraper</li><li class="meta">${t("Keine Treffer. Anders schreiben oder die Spielnummer von screenscraper.fr eingeben.")}</li>`;
    wire();
    return;
  }
  list.innerHTML = localHtml + `<li class="pick-head">ScreenScraper</li>` + hits.map((h, i) => `
    <li><button type="button" data-hit="${i}">
      <strong>${esc(h.title ?? "?")}</strong>
      <span class="meta">${[h.year, h.publisher, tGenres(h.genres), `#${h.id}`].filter(Boolean).map(esc).join(" · ")}</span>
      ${h.synopsis ? `<span class="hit-text">${esc(h.synopsis)}</span>` : ""}
    </button></li>`).join("");
  wire();
  list.querySelectorAll("button[data-hit]").forEach((btn) => btn.addEventListener("click", async () => {
    const hit = hits[Number(btn.dataset.hit)];
    $("#pick-dialog").close();
    try { await assignGame(target, hit.id); } catch (e) { showError(e); }
  }));
}

// Angaben eines Eintrags der Bibliothek übernehmen – die Fassungen stehen danach in einer Zeile.
async function copyMeta(hit, sha1s) {
  await invoke("copy_meta", { from: hit.sha1, to: sha1s });
  setStatus(t("Angaben von „{title}“ übernommen.", { title: hit.title }));
  currentGroup = [];
  if (marked.size > 1) {
    marked.clear();
    selectedSha1 = sha1s[0];
  }
  await refreshAfterChange();
}

$("#pick-form").addEventListener("submit", (ev) => {
  ev.preventDefault();
  runGameSearch().catch(showError);
});
$("#pick-close").addEventListener("click", () => $("#pick-dialog").close());

// ---------------------------------------------------------------- Vorschläge an ScreenScraper

// Felder, die vorgeschlagen werden können: [Typ bei ScreenScraper, Beschriftung, mehrzeilig, braucht Region/Sprache].
// [Typ bei ScreenScraper, Bezeichnung, Eingabe (text, area, players, genres), braucht Region/Sprache]
const PROPOSE_FIELDS = [
  ["name", "Name", "text", "region"],
  ["editeur", "Herausgeber", "text", ""],
  ["developpeur", "Entwickler", "text", ""],
  ["datessortie", "Erscheinungsdatum", "text", "region"],
  ["players", "Spieler", "players", ""],
  ["genres", "Genre", "genres", ""],
  ["description", "Beschreibung", "area", "lang"],
];

// Name eines Listeneintrags (Genre, Spieleranzahl) in der Sprache der Beschreibungen.
function choiceLabel(c) {
  const n = c.names;
  return n[scraperSettings?.language] || n.en || n.fr || n[""] || c.id;
}

const splitList = (v) => String(v ?? "").split(",").map((x) => x.trim()).filter(Boolean);
const sameSet = (a, b) => a.length === b.length && a.every((x) => b.some((y) => y.toLowerCase() === x.toLowerCase()));

// Genres als Liste zum Anhaken: Hauptgenres, darunter eingerückt ihre Untergenres. Wert = französischer
// Name, so verlangt ihn ScreenScraper.
function fillGenres(row, genres, mine) {
  const wanted = mine.map((g) => g.toLowerCase());
  const byId = new Map(genres.map((g) => [g.id, g]));
  const full = (g) => {
    const p = byId.get(g.parent);
    const l = choiceLabel(g);
    return p && !l.startsWith(choiceLabel(p)) ? `${choiceLabel(p)} / ${l}` : l;
  };
  const sorted = (list) => list.sort((a, b) => choiceLabel(a).localeCompare(choiceLabel(b)));
  const mains = sorted(genres.filter((g) => !g.parent || !byId.has(g.parent)));
  const item = (g, sub) => `<label class="inline${sub ? " sub" : ""}" data-search="${esc(full(g).toLowerCase())}">
    <input type="checkbox" value="${esc(g.names.fr || choiceLabel(g))}"${wanted.includes(full(g).toLowerCase()) || wanted.includes(choiceLabel(g).toLowerCase()) ? " checked" : ""}> ${esc(choiceLabel(g))}</label>`;
  row.querySelector(".genre-pick").innerHTML = mains.map((m) =>
    item(m, false) + sorted(genres.filter((g) => g.parent === m.id)).map((g) => item(g, true)).join("")).join("");
  const known = genres.flatMap((g) => [full(g).toLowerCase(), choiceLabel(g).toLowerCase()]);
  const unknown = mine.filter((g) => !known.includes(g.toLowerCase()));
  row.querySelector("[data-hint]").textContent = unknown.length
    ? t("Nicht in der Liste von ScreenScraper: {list}", { list: unknown.join(", ") }) : "";
}

function fillPlayers(row, players, mine) {
  const sel = row.querySelector("select");
  const names = players.map((p) => p.names[""] || choiceLabel(p));
  sel.innerHTML = `<option value="">–</option>` + names.map((n) => `<option${n === mine ? " selected" : ""}>${esc(n)}</option>`).join("");
  row.querySelector("[data-hint]").textContent = mine && !names.includes(mine)
    ? t("Nicht in der Liste von ScreenScraper: {list}", { list: mine }) : "";
}

// Gewählter Wert einer Zeile: Text, Spieleranzahl oder Liste der angehakten Genres.
function propValue(row) {
  if (row.dataset.typ === "genres") return [...row.querySelectorAll(".genre-pick input:checked")].map((i) => i.value);
  return row.querySelector("[data-value]").value.trim();
}
let proposeGame = null;

function propLabel(typ) {
  const f = PROPOSE_FIELDS.find(([t2]) => t2 === typ);
  return f ? t(f[1]) : t({ ss: "Spielszene", sstitle: "Titelbild", "box-2D": "Verpackung" }[typ] ?? typ);
}

async function openPropose(d) {
  const r = d.row;
  const m = d.meta;
  proposeGame = m.game_id;
  $("#propose-title").textContent = t("Vorschlag an ScreenScraper – Spiel #{id}", { id: m.game_id });
  $("#propose-lang").value = LANG === "de" ? "de" : "en";
  $("#propose-source").value = "";
  $("#propose-result").innerHTML = "";
  const mine = { name: m.title ?? r.title, editeur: r.publisher, developpeur: m.developer, datessortie: r.year, description: m.synopsis, players: m.players ?? "" };
  const mineGenres = splitList(r.genres);
  const input = (typ, kind) => ({
    area: `<textarea rows="5" data-value>${esc(mine[typ] ?? "")}</textarea>`,
    players: `<select data-value><option value="">${t("lade …")}</option></select><div class="meta" data-hint></div>`,
    genres: `<input class="genre-filter" placeholder="${t("Genres filtern …")}"><div class="genre-pick">${t("lade …")}</div>
      <div class="meta">${t("Jedes angehakte Genre wird einzeln vorgeschlagen.")}</div><div class="meta" data-hint></div>`,
  })[kind] ?? `<input data-value value="${esc(mine[typ] ?? "")}">`;
  $("#propose-fields").innerHTML = PROPOSE_FIELDS.map(([typ, label, kind]) => `
    <div class="prop-row" data-typ="${typ}">
      <label class="inline"><input type="checkbox" data-check> <strong>${t(label)}</strong></label>
      ${input(typ, kind)}
      <div class="meta now" data-now>${t("Bei ScreenScraper: lade …")}</div>
    </div>`).join("")
    + d.media.filter(([, file]) => file.startsWith("own_")).map(([kind, file]) => `
    <div class="prop-row" data-media="${esc(kind)}" data-file="${esc(file)}">
      <label class="inline"><input type="checkbox" data-check checked>
        <strong>${t({ ss: "Spielszene", sstitle: "Titelbild", "box-2D": "Verpackung" }[kind] ?? kind)}</strong> – ${t("eigenes Bild")}</label>
      <div class="meta now" data-now></div>
    </div>`).join("");
  showProposalLog().catch(() => {});
  $("#propose-dialog").showModal();
  const genreRow = $("#propose-fields .prop-row[data-typ=genres]");
  const playerRow = $("#propose-fields .prop-row[data-typ=players]");
  genreRow.querySelector(".genre-filter").addEventListener("input", (ev) => {
    const q = ev.target.value.trim().toLowerCase();
    genreRow.querySelectorAll(".genre-pick label").forEach((l) => { l.hidden = !!q && !l.dataset.search.includes(q); });
  });
  genreRow.querySelector(".genre-pick").addEventListener("change", () => { genreRow.querySelector("[data-check]").checked = true; });
  playerRow.querySelector("select").addEventListener("change", () => { playerRow.querySelector("[data-check]").checked = true; });
  const choices = invoke("proposal_choices").then((c) => {
    fillGenres(genreRow, c.genres, mineGenres);
    fillPlayers(playerRow, c.players, mine.players);
  }).catch((e) => {
    genreRow.querySelector(".genre-pick").textContent = String(e);
    playerRow.querySelector("[data-hint]").textContent = String(e);
  });
  try {
    const [now] = await Promise.all([invoke("game_now", { gameId: m.game_id }), choices]);
    const theirs = { name: now.title, editeur: now.publisher, developpeur: now.developer, datessortie: now.year, description: now.synopsis, players: now.players, genres: (now.genres ?? []).join(", ") };
    document.querySelectorAll("#propose-fields .prop-row[data-typ]").forEach((row) => {
      const v = theirs[row.dataset.typ];
      row.querySelector("[data-now]").textContent = v ? t("Bei ScreenScraper: {v}", { v: v.length > 160 ? `${v.slice(0, 160)}…` : v }) : t("Bei ScreenScraper: (leer)");
      // Vorschlagen, was fehlt oder abweicht.
      if (row.dataset.typ === "genres") {
        const picked = [...row.querySelectorAll(".genre-pick input:checked")].map((i) => i.parentElement.textContent.trim());
        row.querySelector("[data-check]").checked = picked.length > 0 && !sameSet(mineGenres, splitList(v));
        return;
      }
      const mineV = propValue(row);
      row.querySelector("[data-check]").checked = !!mineV && mineV !== (v ?? "").trim();
    });
    document.querySelectorAll("#propose-fields .prop-row[data-media]").forEach((row) => {
      row.querySelector("[data-now]").textContent = now.media.includes(row.dataset.media)
        ? t("ScreenScraper hat schon ein Bild dieser Art.") : t("ScreenScraper hat noch kein Bild dieser Art.");
    });
  } catch (e) {
    document.querySelectorAll("#propose-fields [data-now]").forEach((el) => { el.textContent = String(e); });
  }
}

async function showProposalLog() {
  const log = await invoke("proposals", { gameId: proposeGame });
  $("#propose-log").innerHTML = log.length ? `<h4>${t("Bisher vorgeschlagen")}</h4><ul>${log.map((p) =>
    `<li><span class="${p.ok ? "" : "bad"}">${esc(propLabel(p.typ))}</span> · ${esc(fmtDate(p.sent_at))} · <span class="meta">${esc(p.response.slice(0, 160))}</span></li>`).join("")}</ul>` : "";
}

$("#propose-form").addEventListener("submit", async (ev) => {
  ev.preventDefault();
  const region = $("#propose-region").value;
  const langue = $("#propose-lang").value;
  const source = $("#propose-source").value.trim();
  const items = [];
  document.querySelectorAll("#propose-fields .prop-row").forEach((row) => {
    if (!row.querySelector("[data-check]").checked) return;
    if (row.dataset.media) {
      items.push({ kind: "media", game_id: proposeGame, typ: row.dataset.media, region, file_name: row.dataset.file, source });
      return;
    }
    const typ = row.dataset.typ;
    if (typ === "genres") {
      // Mehrfachauswahl: jedes Genre als eigener Vorschlag.
      propValue(row).forEach((text) => items.push({ kind: "info", game_id: proposeGame, typ, region: "", langue: "", text, source }));
      return;
    }
    let text = propValue(row);
    if (!text) return;
    // ScreenScraper erwartet jjjj-mm-tt; nur ein Jahr → 1. Januar.
    if (typ === "datessortie" && /^\d{4}$/.test(text)) text = `${text}-01-01`;
    const needs = PROPOSE_FIELDS.find(([t2]) => t2 === typ)[3];
    items.push({ kind: "info", game_id: proposeGame, typ, region: needs === "region" ? region : "", langue: needs === "lang" ? langue : "", text, source });
  });
  if (!items.length) {
    $("#propose-result").textContent = t("Nichts angehakt.");
    return;
  }
  const answer = await invoke("plugin:dialog|message", {
    message: t("{n} Vorschläge mit deinem ScreenScraper-Konto senden? Die Moderatoren prüfen sie, bevor sie erscheinen.", { n: items.length }),
    title: t("Bei ScreenScraper vorschlagen"), kind: "info",
    buttons: { OkCancelCustom: [t("Senden"), t("Abbrechen")] },
  });
  if (!confirmed(answer, t("Senden"))) return;
  const btn = $("#propose-send");
  btn.disabled = true;
  $("#propose-result").textContent = t("Sende …");
  try {
    const res = await invoke("propose", { items });
    $("#propose-result").innerHTML = `<ul>${res.map((r) =>
      `<li class="${r.ok ? "ok" : "bad"}"><strong>${esc(propLabel(r.typ))}</strong>: ${esc(r.text || (r.ok ? t("gesendet") : t("Fehler")))}</li>`).join("")}</ul>`;
    await showProposalLog();
  } catch (e) {
    $("#propose-result").textContent = String(e);
  } finally {
    btn.disabled = false;
  }
});
$("#propose-close").addEventListener("click", () => $("#propose-dialog").close());

// ---------------------------------------------------------------- Eigene Angaben

const OWN_FIELDS = ["title", "year", "publisher", "developer", "genres", "players", "synopsis", "synopsis_en"];

// Beschreibung in der Sprache der Oberfläche; fehlt sie, die andere mit Hinweis. ScreenScraper-Texte kommen in der dort
// eingestellten Sprache und stehen immer in `synopsis` (ohne Hinweis).
function synopsisHtml(m) {
  const own = (m.provider ?? "").startsWith("own") || m.provider === "shared";
  const de = m.synopsis ?? "", en = m.synopsis_en ?? "";
  const wanted = LANG === "en" ? en : de, other = LANG === "en" ? de : en;
  if (!own && !en) return de ? `<p class="synopsis">${esc(de)}</p>` : "";
  const text = wanted || other;
  if (!text) return "";
  const note = wanted ? "" : `<p class="meta">${LANG === "en" ? t("Beschreibung nur auf Deutsch vorhanden.") : t("Beschreibung nur auf Englisch vorhanden.")}</p>`;
  return `<p class="synopsis" lang="${wanted ? LANG : (LANG === "en" ? "de" : "en")}">${esc(text)}</p>${note}`;
}
const OWN_KINDS = [["sstitle", "Titelbild"], ["ss", "Spielszene"], ["box-2D", "Verpackung"]];
let ownTarget = null;
let ownSaved = false;
// „Eigene Fassung schreiben“: leerer Dialog, gespeichert als selbst geschrieben (teilbar).
let ownFresh = false;

// d: Details des gezeigten Eintrags (Vorbelegung, Bilder); bei mehreren markierten null.
function openOwnEditor(sha1s, d, title = "", fresh = false) {
  ownTarget = sha1s;
  ownSaved = false;
  ownFresh = fresh;
  const r = d?.row ?? {};
  const m = d?.meta?.status === "ok" && !fresh ? d.meta : {};
  const values = fresh ? { title: r.title } : {
    title: m.title ?? r.title ?? searchTextFor(title), year: r.year, publisher: r.publisher, developer: m.developer,
    genres: r.genres, players: m.players, synopsis: m.synopsis, synopsis_en: m.synopsis_en,
  };
  for (const f of OWN_FIELDS) $(`#own-${f}`).value = values[f] ?? "";
  $("#own-count").textContent = sha1s.length > 1 ? t("Gilt für alle {n} Fassungen.", { n: sha1s.length }) : "";
  $("#own-clear").hidden = !d?.meta?.provider?.startsWith("own");
  $("#own-heading").textContent = fresh ? t("Eigene Fassung schreiben")
    : d?.meta?.status === "ok" ? t("Angaben bearbeiten") : t("Angaben selbst eintragen");
  // Eigene Fassung: ohne Bilder von ScreenScraper (sie fallen beim Speichern weg).
  renderOwnImages((d?.media ?? []).filter(([, f]) => !(fresh && f.startsWith("ss_"))));
  $("#own-dialog").showModal();
}

// Titelbild und Spielszene lassen sich gegeneinander umordnen (z. B. versehentlich falsch gespeichert).
const OWN_MOVE = { sstitle: ["ss", "Als Spielszene"], ss: ["sstitle", "Als Titelbild"] };

// Bildbereich des Dialogs; media: Liste [Bildart, Datei].
function renderOwnImages(mediaList) {
  const media = Object.fromEntries(mediaList);
  // Titelbild und Spielszene lassen sich auch vom Bildschirm einer Ultimate holen.
  const shooters = devices.filter(canShoot);
  const prev = Number($("#own-shot-device")?.value);
  const sel = [prev, storedDeviceId()].find((id) => shooters.some((x) => x.id === id)) ?? shooters[0]?.id;
  const shotBtn = (kind) => shooters.length && kind !== "box-2D"
    ? `<button type="button" class="small" data-img="shot" title="${t("Was gerade auf dem Gerät zu sehen ist, als dieses Bild speichern (320 × 200) – nur mit Netzwerkkabel am Gerät")}">${t("Bildschirmfoto")}</button>` : "";
  $("#own-images").innerHTML = OWN_KINDS.map(([kind, label]) => `
    <div class="own-image" data-kind="${kind}">
      <div class="thumb">${media[kind] ? `<img alt="">` : `<span class="meta">${t("kein Bild")}</span>`}</div>
      <div class="meta">${t(label)}</div>
      ${media[kind]?.startsWith("ss_") ? `<div class="meta ss-note">${t("von ScreenScraper – wird nicht geteilt")}</div>` : ""}
      <button type="button" class="small" data-img="pick">${t("Wählen …")}</button>
      ${shotBtn(kind)}
      ${media[kind] && OWN_MOVE[kind] ? `<button type="button" class="small" data-img="move" data-to="${OWN_MOVE[kind][0]}"
        title="${t("Liegt dort schon ein Bild, tauschen beide den Platz.")}">${t(OWN_MOVE[kind][1])}</button>` : ""}
      ${media[kind] ? `<button type="button" class="small link danger" data-img="remove">${t("Entfernen")}</button>` : ""}
    </div>`).join("") + (shooters.length > 1 ? `
    <label class="own-shot meta">${t("Bildschirmfoto von")}
      <select id="own-shot-device">${shooters.map((x) =>
        `<option value="${x.id}" ${x.id === sel ? "selected" : ""}>${esc(x.name)}</option>`).join("")}</select>
    </label>` : "");
  for (const [kind] of OWN_KINDS) {
    if (!media[kind]) continue;
    invoke("media_data", { file: media[kind] }).then((url) => {
      const img = $(`#own-images [data-kind="${kind}"] img`);
      if (img) img.src = url;
    }).catch(() => {});
  }
}

// Bilder nach einer Änderung neu aus der Datenbank zeigen (bei mehreren Fassungen die erste).
async function reloadOwnImages() {
  renderOwnImages((await invoke("details", { sha1: ownTarget[0] })).media);
}

function ownValues() {
  return Object.fromEntries(OWN_FIELDS.map((f) => [f, $(`#own-${f}`).value]));
}

async function saveOwn() {
  await invoke("save_own_meta", { sha1s: ownTarget, meta: ownValues(), fresh: ownFresh });
  ownSaved = true;
}

$("#own-images").addEventListener("click", async (ev) => {
  const btn = ev.target.closest("button[data-img]");
  if (!btn) return;
  const kind = btn.closest("[data-kind]").dataset.kind;
  if (btn.dataset.img === "shot") {
    await ownShot(btn, kind).catch(showError);
    return;
  }
  if (btn.dataset.img === "move") {
    try {
      if (!ownSaved) await saveOwn();
      await invoke("move_image", { sha1s: ownTarget, from: kind, to: btn.dataset.to });
      await reloadOwnImages();
    } catch (e) {
      showError(e);
    }
    return;
  }
  try {
    let path = null;
    if (btn.dataset.img === "pick") {
      path = await invoke("plugin:dialog|open", { options: {
        multiple: false, title: t("Bild wählen"),
        filters: [{ name: t("Bilder"), extensions: ["png", "jpg", "jpeg", "gif", "webp"] }],
      } });
      if (!path) return;
    }
    // Bilder hängen an den Angaben: Felder vorher speichern.
    if (!ownSaved) await saveOwn();
    await invoke("set_own_image", { sha1s: ownTarget, kind, path });
    await reloadOwnImages();
  } catch (e) {
    showError(e);
  }
});

// Bildschirmfoto direkt im Dialog: Vorschau statt „gespeichert“.
async function ownShot(btn, kind) {
  const deviceId = Number($("#own-shot-device")?.value) || devices.find(canShoot)?.id;
  const dev = devices.find((x) => x.id === deviceId);
  if (!ownSaved) await saveOwn();
  btn.disabled = true;
  setStatus(t("Nehme das Bild von {name} auf …", { name: dev?.name ?? "?" }));
  try {
    await invoke("capture_screen", { deviceId, sha1s: ownTarget, kind, crop: true });
    await reloadOwnImages();
    setStatus(t("Bildschirmfoto gespeichert."));
  } finally {
    btn.disabled = false;
  }
}

$("#own-form").addEventListener("submit", async (ev) => {
  ev.preventDefault();
  try {
    await saveOwn();
    $("#own-dialog").close();
    if (marked.size > 1) {
      marked.clear();
      selectedSha1 = ownTarget[0];
      currentGroup = [];
    }
    await refreshAfterChange();
  } catch (e) {
    showError(e);
  }
});
$("#own-close").addEventListener("click", async () => {
  $("#own-dialog").close();
  if (ownSaved) await refreshAfterChange().catch(showError);
});
$("#own-clear").addEventListener("click", async () => {
  const answer = await invoke("plugin:dialog|message", {
    message: t("Eigene Angaben und Bilder löschen?"), title: t("Eigene Angaben"), kind: "warning",
    buttons: { OkCancelCustom: [t("Löschen"), t("Abbrechen")] },
  });
  if (!confirmed(answer, t("Löschen"))) return;
  try {
    await invoke("clear_meta", { sha1s: ownTarget });
    $("#own-dialog").close();
    await refreshAfterChange();
  } catch (e) {
    showError(e);
  }
});

// ---------------------------------------------------------------- ScreenScraper

let scraperSettings = null;

/** Ist ein ScreenScraper-Zugang eingerichtet? */
function ssReady() {
  const s = scraperSettings;
  return !!s && !!(s.dev_builtin || (s.dev_id && s.has_dev_password)) && !!s.user && s.has_password;
}

async function loadScraperSettings() {
  scraperSettings = await invoke("get_scraper_settings");
  const s = scraperSettings;
  const devOk = s.dev_builtin || (s.dev_id && s.has_dev_password);
  const ready = devOk && s.user && s.has_password;
  $("#scraper-state").textContent = ready
    ? t("Angemeldet als {user}", { user: s.user })
    : !devOk ? t("Entwickler-Kennung fehlt – siehe Einstellungen.") : t("Kein Zugang eingerichtet (freiwillig).");
  $("#btn-scrape-list").disabled = !ready || scraping || scanning;
  $("#btn-community-submit").hidden = !s.submit_url;
  $("#community-web").hidden = !communityWebUrl();
  $("#btn-scrape-all").disabled = !ready || scraping || scanning;
}

function openSettings() {
  const s = scraperSettings;
  $("#ss-user").value = s.user;
  $("#ss-password").value = "";
  $("#ss-password").type = "password";
  $("#ss-show-password").checked = false;
  $("#ss-password").placeholder = s.has_password ? t("gespeichert – nur zum Ändern ausfüllen") : "";
  $("#ui-language").value = storedUiLang();
  $("#ss-language").value = s.language;
  $("#ss-name-fallback").checked = s.name_fallback;
  $("#ss-threads").value = String(s.threads ?? 0);
  $("#keep-awake").checked = s.keep_awake;
  $("#preferred-source").value = s.preferred_source;
  const ownData = s.community_url !== s.default_community_url;
  $("#community-src").value = ownData ? "own" : "std";
  $("#community-url").value = ownData ? s.community_url : "";
  $("#submit-src").value = !s.submit_url ? "off" : s.submit_url === s.default_submit_url ? "std" : "own";
  $("#submit-url").value = $("#submit-src").value === "own" ? s.submit_url : "";
  showServerRows();
  $("#community-auto").checked = s.community_auto;
  $("#update-check").checked = s.update_check;
  $("#auto-title").checked = s.auto_title;
  $("#auto-title-delay").value = String(s.auto_title_delay ?? 25);
  $("#ss-dev").hidden = s.dev_builtin;
  $("#ss-devid").value = s.dev_id;
  $("#ss-devpassword").value = "";
  $("#ss-devpassword").placeholder = s.has_dev_password ? t("gespeichert – nur zum Ändern ausfüllen") : "";
  $("#ss-softname").textContent = s.softname;
  setResult("", "");
  $("#settings").showModal();
}

function setResult(text, cls) {
  const el = $("#ss-result");
  el.textContent = text;
  el.className = `meta ${cls}`;
}

async function saveSettings() {
  await invoke("save_scraper_settings", {
    input: {
      user: $("#ss-user").value,
      password: $("#ss-password").value || null,
      dev_id: scraperSettings.dev_builtin ? null : $("#ss-devid").value,
      dev_password: $("#ss-devpassword").value || null,
      language: $("#ss-language").value,
      name_fallback: $("#ss-name-fallback").checked,
      threads: Number($("#ss-threads").value),
      keep_awake: $("#keep-awake").checked,
      preferred_source: $("#preferred-source").value,
      // Standard bzw. leeres eigenes Feld → eingebaute Adresse (wird nicht gespeichert); „aus“ → leer.
      community_url: ($("#community-src").value === "own" && $("#community-url").value.trim()) || scraperSettings.default_community_url,
      submit_url: { off: "", own: $("#submit-url").value.trim() || scraperSettings.default_submit_url }[$("#submit-src").value]
        ?? scraperSettings.default_submit_url,
      community_auto: $("#community-auto").checked,
      update_check: $("#update-check").checked,
      auto_title: $("#auto-title").checked,
      auto_title_delay: Number($("#auto-title-delay").value) || 25,
    },
  });
  await loadScraperSettings();
}

// Adressfeld nur zeigen, wenn „Eigener Server“ gewählt ist
function showServerRows() {
  $("#community-url-row").hidden = $("#community-src").value !== "own";
  $("#submit-url-row").hidden = $("#submit-src").value !== "own";
}
$("#community-src").addEventListener("change", showServerRows);
$("#submit-src").addEventListener("change", showServerRows);

$("#btn-settings").addEventListener("click", openSettings);

// ---------------------------------------------------------------- Anleitung und Über

async function openHelp(anchor = "") {
  const body = $("#help-body");
  if (!body.dataset.loaded) {
    try {
      body.innerHTML = await (await fetch(LANG === "de" ? "help.de.html" : "help.en.html")).text();
      body.dataset.loaded = "1";
    } catch (e) {
      body.textContent = String(e);
    }
  }
  $("#help-dialog").showModal();
  if (anchor) body.querySelector(anchor)?.scrollIntoView();
  else body.scrollTop = 0;
}

// Sprungmarken im Inhaltsverzeichnis: im Dialog scrollen statt die Seite zu wechseln.
$("#help-body").addEventListener("click", (ev) => {
  const a = ev.target.closest("a[href^='#']");
  if (!a) return;
  ev.preventDefault();
  $("#help-body").querySelector(a.getAttribute("href"))?.scrollIntoView({ behavior: "smooth" });
});

$("#btn-help").addEventListener("click", () => openHelp().catch(showError));
document.addEventListener("keydown", (ev) => {
  if (ev.key === "F1" || ((ev.metaKey || ev.ctrlKey) && ev.key === "?")) {
    ev.preventDefault();
    openHelp().catch(showError);
  }
});

$("#btn-about").addEventListener("click", async () => {
  try { $("#about-version").textContent = await invoke("plugin:app|version"); } catch { /* bleibt leer */ }
  $("#about-dialog").showModal();
});
$("#about-dialog").addEventListener("click", (ev) => {
  const a = ev.target.closest("a[data-url]");
  if (!a) return;
  ev.preventDefault();
  invoke("plugin:opener|open_url", { url: a.dataset.url }).catch(showError);
});
$("#about-backup").addEventListener("click", async () => {
  try {
    const day = new Date().toISOString().slice(0, 10);
    const path = await invoke("plugin:dialog|save", { options: { defaultPath: `Breadbin-Sicherung-${day}.zip`, filters: [{ name: "ZIP", extensions: ["zip"] }] } });
    if (!path) return;
    setStatus(t("Sichere die Bibliothek …"));
    const r = await invoke("backup_library", { path });
    setStatus(t("Bibliothek gesichert: {n} Bilder, {size}.", { n: nf(r.images), size: fmtSize(r.bytes) }));
  } catch (e) { showError(e); }
});

$("#about-restore").addEventListener("click", async () => {
  try {
    const path = await invoke("plugin:dialog|open", { options: { filters: [{ name: "ZIP", extensions: ["zip"] }] } });
    if (!path) return;
    const answer = await invoke("plugin:dialog|message", {
      message: t("Die jetzige Bibliothek wird durch die Sicherung ersetzt (Quellen, Geräte, eigene Angaben, Listen, Notizen, Einstellungen). Die bisherige Datenbank bleibt im Datenordner als „spiele.vor-wiederherstellung.sqlite“ liegen. Passwörter liegen im Schlüsselbund und bleiben, wie sie sind."),
      title: t("Sicherung zurückspielen"), kind: "warning", buttons: { OkCancelCustom: [t("Zurückspielen"), t("Abbrechen")] },
    });
    if (!confirmed(answer, t("Zurückspielen"))) return;
    setStatus(t("Spiele die Sicherung zurück …"));
    const n = await invoke("restore_library", { path });
    await invoke("plugin:dialog|message", { message: t("Sicherung zurückgespielt ({n} Bilder). Breadbin lädt die Ansicht neu.", { n: nf(n) }), title: t("Sicherung zurückspielen"), kind: "info" });
    location.reload();
  } catch (e) { showError(e); }
});

$("#about-data").addEventListener("click", async () => {
  try {
    await invoke("plugin:opener|reveal_item_in_dir", { paths: [await invoke("data_path")] });
  } catch (e) {
    showError(e);
  }
});
$("#about-help").addEventListener("click", () => {
  $("#about-dialog").close();
  openHelp().catch(showError);
});
document.querySelectorAll(".open-settings").forEach((b) => b.addEventListener("click", openSettings));
$("#ss-show-password").addEventListener("change", (ev) => {
  $("#ss-password").type = ev.target.checked ? "text" : "password";
});
$("#ss-close").addEventListener("click", () => $("#settings").close());
$("#ss-save").addEventListener("click", async () => {
  try {
    await saveSettings();
    $("#settings").close();
    // Neue Sprache: Seite neu laden, damit alle Texte wechseln.
    if (setUiLang($("#ui-language").value)) location.reload();
  } catch (e) {
    setResult(String(e), "bad");
  }
});
$("#ss-test").addEventListener("click", async () => {
  setResult(t("Prüfe …"), "");
  try {
    await saveSettings();
    const u = await invoke("test_scraper");
    setResult(t("Zugang in Ordnung. Heute {today} von {max} Abfragen genutzt, {threads} gleichzeitig erlaubt.",
      { today: nf(u.requests_today), max: nf(u.max_requests_per_day), threads: u.max_threads }), "ok");
  } catch (e) {
    setResult(String(e), "bad");
  }
});
$("#settings").addEventListener("click", (ev) => {
  const a = ev.target.closest("a[data-url]");
  if (!a) return;
  ev.preventDefault();
  invoke("plugin:opener|open_url", { url: a.dataset.url }).catch(showError);
});

async function startScrape({ sha1s = null, force = false, retryNotfound = false } = {}) {
  await invoke("start_scrape", { sha1s, force, retryNotfound });
  scraping = true;
  $("#btn-cancel").hidden = false;
  setStatus(t("Abruf startet …"));
  await loadScraperSettings();
}

$("#btn-scrape-list").addEventListener("click", () => {
  if (!lastRows.length) return;
  startScrape({ sha1s: lastRows.flatMap((r) => r.variants) }).catch(showError);
});
$("#btn-scrape-all").addEventListener("click", () => startScrape({}).catch(showError));

listen("scrape-progress", async ({ payload: ev }) => {
  const p = ev.progress;
  if (!ev.finished) {
    setStatus(t("ScreenScraper: {done} von {total} – {found} gefunden, {notfound} nicht gefunden – {current}",
      { done: nf(p.done), total: nf(p.total), found: nf(p.found), notfound: nf(p.not_found), current: p.current })
      + (p.threads > 1 ? t(" · {n} gleichzeitig", { n: p.threads }) : ""));
    $("#ss-progress-text").textContent = t("Läuft: {done} von {total} – {found} gefunden", { done: nf(p.done), total: nf(p.total), found: nf(p.found) });
    if (p.total) $("#ss-bar").style.width = `${(p.done / p.total) * 100}%`;
    // Fehler schon während des Abrufs einsehbar (setStatus blendet den Knopf sonst jedes Mal aus).
    if (p.errors.length) showErrorsButton(t("Fehler beim Abruf von ScreenScraper"), p.errors);
    return;
  }
  loadStats().catch(() => {});
  scraping = false;
  $("#btn-cancel").hidden = true;
  await loadScraperSettings();
  if (ev.error) {
    showError(ev.error);
    return;
  }
  const parts = [t("{n} gefunden", { n: nf(p.found) }), t("{n} nicht gefunden", { n: nf(p.not_found) })];
  if (p.errors.length) parts.push(t("{n} Fehler", { n: p.errors.length }));
  const end = p.cancelled ? t("abgebrochen") : p.stopped ? t("angehalten") : t("fertig");
  setStatus(t("ScreenScraper {end}: {parts}{rest}", { end, parts: parts.join(", "), rest: p.total === 0 ? t(" (nichts offen)") : "" }));
  // Erst die Meldung (sie setzt die Statuszeile), dann den Knopf, sonst blendet die Meldung ihn aus.
  if (p.stopped) showError(p.stopped);
  if (p.errors.length) showErrorsButton(t("Fehler beim Abruf von ScreenScraper"), p.errors);
  await refreshAfterChange();
});

// ---------------------------------------------------------------- Ultimate-Geräte

function storedDeviceId() {
  try { return Number(localStorage.getItem("launchDevice")) || null; } catch { return null; }
}

async function loadDevices() {
  devices = await invoke("list_devices");
  const ul = $("#devices");
  $("#devices-count").textContent = devices.length ? nf(devices.length) : "";
  ul.innerHTML = devices.length ? devices.map((d) => `
    <li data-id="${d.id}" class="row">
      <div class="row-main" title="${esc(t("Startordner: {dir}", { dir: d.upload_dir }))}">
        <div class="label">${esc(d.name)}<span class="status" data-status>${t("prüfe …")}</span></div>
        <div class="meta">${esc(d.host)} · ${esc(d.product)}<span data-fw></span><span class="hint-mark" data-hints hidden>⚠</span></div>
      </div>
      <button data-act="wake" class="small icon" hidden title="${t("Einschalten")}" aria-label="${t("Einschalten")}">⏻</button>
      <button data-act="off" class="small icon" hidden title="${t("Ausschalten")}" aria-label="${t("Ausschalten")}">⏻</button>
      <details class="row-menu">
        <summary title="${t("Weitere Aktionen")}" aria-label="${t("Weitere Aktionen")}">⋯</summary>
        <div class="menu">
          <div class="meta menu-info">${esc(t("Startordner: {dir}", { dir: d.upload_dir }))}</div>
          <button data-act="folder">${t("Ordner als Quelle")}</button>
          <button data-act="edit">${t("Bearbeiten")}</button>
          <button data-act="saves" title="${t("Disketten, die sich im Startordner seit dem Hinkopieren geändert haben (Spielstände), auf den Rechner holen")}">${t("Spielstände zurückholen …")}</button>
          <button data-act="clean" title="${t("Dateien löschen, die Breadbin zum Starten in den Startordner kopiert hat")}">${t("Startordner leeren")}</button>
          <button data-act="remove" class="danger">${t("Entfernen")}</button>
        </div>
      </details>
    </li>`).join("")
    : `<li class="empty">${t("Noch kein Gerät. Mit „+ Gerät“ einen Ultimate im Netzwerk hinzufügen, um Spiele dort zu starten.")}</li>`;
  devices.forEach((d) => refreshDeviceStatus(d.id));
}

const deviceStatus = {};

// Status nachladen, ohne die Liste neu aufzubauen (Knöpfe passend ein-/ausblenden).
async function refreshDeviceStatus(id) {
  const li = $(`#devices li[data-id="${id}"]`);
  if (!li) return null;
  const el = li.querySelector("[data-status]");
  el.className = "status";
  el.textContent = t("prüfe …");
  try {
    const st = await invoke("device_status", { id });
    deviceStatus[id] = st;
    el.className = `status ${st.online ? "on" : "off"}`;
    el.textContent = st.online ? t("an") : t("aus");
    li.querySelector("[data-act=wake]").hidden = st.online || !st.can_wake;
    li.querySelector("[data-act=off]").hidden = !st.online || !st.can_power_off;
    li.querySelector("[data-fw]").textContent = st.firmware ? ` · ${t("Firmware {v}", { v: st.firmware })}` : "";
    const hints = li.querySelector("[data-hints]");
    hints.hidden = !st.hints?.length;
    hints.title = (st.hints ?? []).join("\n\n");
    return st;
  } catch {
    el.textContent = t("unbekannt");
    return null;
  }
}

async function wakeDevice(dev) {
  setStatus(t("Schalte {name} ein …", { name: dev.name }));
  const secs = await invoke("wake_device", { id: dev.id });
  setStatus(t("{name} ist nach {secs} s eingeschaltet.", { name: dev.name, secs: secs.toFixed(0) }));
  await refreshDeviceStatus(dev.id);
}

let editingDevice = null;

function setDevResult(text, cls) {
  $("#dev-result").textContent = text;
  $("#dev-result").className = `meta ${cls}`;
}

function fillDrives(listSel, drives) {
  $(listSel).innerHTML = drives.map((d) => `<option value="/${esc(d)}/">`).join("");
}

function openDeviceDialog(dev) {
  editingDevice = dev ? { ...dev } : { id: 0, name: "", host: "", product: "", upload_dir: "", wifi_mac: "" };
  $("#dev-found").innerHTML = "";
  $("#dev-title").textContent = dev ? t("Gerät bearbeiten") : t("Gerät hinzufügen");
  $("#dev-host").value = editingDevice.host;
  $("#dev-password").value = "";
  $("#dev-name").value = editingDevice.name;
  $("#dev-upload").value = editingDevice.upload_dir;
  setDevResult("", "");
  $("#device-dialog").showModal();
}

$("#btn-add-device").addEventListener("click", () => openDeviceDialog(null));
$("#dev-close").addEventListener("click", () => $("#device-dialog").close());
$("#dev-discover").addEventListener("click", async () => {
  const ul = $("#dev-found");
  ul.innerHTML = `<li class="meta">${t("Suche im Netz … (einige Sekunden)")}</li>`;
  try {
    const found = await invoke("discover_devices");
    const known = new Set(devices.map((d) => d.host));
    // Ein Gerät mit LAN und WLAN kann unter der anderen Adresse eingetragen sein.
    const knownAs = (f) => [f.host, ...(f.other_hosts ?? [])].find((h) => known.has(h));
    ul.innerHTML = found.length ? found.map((f, i) => `
      <li data-i="${i}"><strong>${esc(f.product)}</strong> – ${esc(f.host)}${f.firmware ? t(", Firmware {fw}", { fw: esc(f.firmware) }) : ""}
        ${f.other_hosts?.length ? `<span class="meta">${t("(auch {hosts})", { hosts: esc(f.other_hosts.join(", ")) })}</span>` : ""}
        ${knownAs(f) === f.host ? `<span class="meta">${t("(schon eingetragen)")}</span>`
          : knownAs(f) ? `<span class="meta">${t("(schon eingetragen als {host})", { host: esc(knownAs(f)) })}</span>` : ""}</li>`).join("")
      : `<li class="meta">${t("Kein eingeschaltetes Gerät gefunden. Geräte mit Netzwerk-Passwort antworten der Suche nicht.")}</li>`;
    ul.querySelectorAll("li[data-i]").forEach((li) => li.addEventListener("click", () => {
      const f = found[Number(li.dataset.i)];
      $("#dev-host").value = f.host;
      if (!$("#dev-name").value) $("#dev-name").value = f.product;
      editingDevice.wifi_mac = f.wifi_mac;
      $("#dev-probe").click();
    }));
  } catch (e) {
    ul.innerHTML = `<li class="meta">${esc(e)}</li>`;
  }
});
$("#dev-probe").addEventListener("click", async () => {
  const host = $("#dev-host").value.trim();
  if (!host) return setDevResult(t("Bitte die Adresse eintragen."), "bad");
  setDevResult(t("Verbinde …"), "");
  try {
    const info = await invoke("probe_device", { host, password: $("#dev-password").value || null });
    editingDevice.product = info.product;
    if (info.wifi_mac) editingDevice.wifi_mac = info.wifi_mac;
    if (!$("#dev-name").value) $("#dev-name").value = info.product;
    fillDrives("#dev-drives", info.drives);
    if (!$("#dev-upload").value) {
      const usb = info.drives.find((d) => d.toUpperCase().startsWith("USB")) ?? info.drives.find((d) => d === "SD") ?? "USB0";
      $("#dev-upload").value = `/${usb}/Breadbin`;
    }
    setDevResult(t("Gefunden: {product}{fw}. Laufwerke: {drives}.", {
      product: info.product, fw: info.firmware ? t(", Firmware {fw}", { fw: info.firmware }) : "",
      drives: info.drives.join(", ") || t("keine"),
    }), "ok");
  } catch (e) {
    setDevResult(String(e), "bad");
  }
});
$("#dev-save").addEventListener("click", async () => {
  try {
    const device = { ...editingDevice, host: $("#dev-host").value.trim(), name: $("#dev-name").value.trim(),
      upload_dir: $("#dev-upload").value.trim() || "/USB0/Breadbin", product: editingDevice.product || "Ultimate" };
    await invoke("save_device", { device, password: $("#dev-password").value || null });
    $("#device-dialog").close();
    await loadDevices();
    if (selectedSha1) await showDetails(selectedSha1);
  } catch (e) {
    setDevResult(String(e), "bad");
  }
});

let folderDevice = null;

$("#devices").addEventListener("click", async (ev) => {
  const btn = ev.target.closest("button[data-act]");
  if (!btn) return;
  const dev = devices.find((d) => d.id === Number(btn.closest("li").dataset.id));
  btn.closest("details")?.removeAttribute("open");
  try {
    if (btn.dataset.act === "wake") {
      btn.disabled = true;
      await wakeDevice(dev).finally(() => { btn.disabled = false; });
    } else if (btn.dataset.act === "off") {
      btn.disabled = true;
      setStatus(t("Schalte {name} aus …", { name: dev.name }));
      const r = await invoke("power_off_device", { id: dev.id }).finally(() => { btn.disabled = false; });
      setStatus(r === "Off" ? t("{name} ist ausgeschaltet.", { name: dev.name })
        : t("{name}: Befehl angenommen, Gerät antwortet aber noch.", { name: dev.name }));
      await refreshDeviceStatus(dev.id);
    } else if (btn.dataset.act === "clean") {
      const answer = await invoke("plugin:dialog|message", {
        message: t("Im Startordner {dir} auf „{name}“ die Dateien löschen, die Breadbin dort zum Starten abgelegt hat? Andere Dateien bleiben erhalten, ebenso Disketten mit Spielstand, bis du sie zurückholst.",
          { dir: dev.upload_dir, name: dev.name }),
        title: t("Startordner leeren"), kind: "warning", buttons: { OkCancelCustom: [t("Löschen"), t("Abbrechen")] },
      });
      if (!confirmed(answer, t("Löschen"))) return;
      const [deleted, kept] = await invoke("clean_upload_dir", { id: dev.id });
      setStatus(t("{name}: {n} Dateien gelöscht{kept}.",
        { name: dev.name, n: deleted, kept: kept ? t(", {n} Dateien behalten (fremde oder mit Spielstand)", { n: kept }) : "" }));
    } else if (btn.dataset.act === "saves") {
      await fetchSaves(dev);
    } else if (btn.dataset.act === "edit") {
      openDeviceDialog(dev);
    } else if (btn.dataset.act === "folder") {
      folderDevice = dev;
      $("#folder-device").textContent = `${dev.name} (${dev.host})`;
      $("#folder-path").value = "/";
      $("#folder-dialog").showModal();
      browseFolder("/");
    } else if (btn.dataset.act === "remove") {
      const answer = await invoke("plugin:dialog|message", {
        message: t("Gerät „{name}“ entfernen? Ordner-Quellen auf dem Gerät bleiben erhalten.", { name: dev.name }),
        title: t("Gerät entfernen"), kind: "warning", buttons: { OkCancelCustom: [t("Entfernen"), t("Abbrechen")] },
      });
      if (!confirmed(answer, t("Entfernen"))) return;
      await invoke("remove_device", { id: dev.id });
      await loadDevices();
      if (selectedSha1) await showDetails(selectedSha1);
    }
  } catch (e) {
    showError(e);
  }
});

$("#folder-close").addEventListener("click", () => $("#folder-dialog").close());

// Ordner auf dem Gerät durchklicken; der gewählte Pfad landet im Eingabefeld.
async function browseFolder(path) {
  $("#folder-path").value = path;
  const parts = path.split("/").filter(Boolean);
  $("#folder-crumbs").innerHTML = [`<button data-path="/">${t("Gerät")}</button>`,
    ...parts.map((p, i) => `/ <button data-path="/${esc(parts.slice(0, i + 1).join("/"))}">${esc(p)}</button>`)].join(" ");
  const ul = $("#folder-list");
  ul.innerHTML = `<li class="meta">${t("Lese Ordner …")}</li>`;
  try {
    const dirs = await invoke("list_device_dirs", { deviceId: folderDevice.id, path });
    if ($("#folder-path").value !== path) return; // inzwischen weitergeklickt
    ul.innerHTML = dirs.length
      ? dirs.map((d) => `<li data-name="${esc(d)}">📁 ${esc(d)}</li>`).join("")
      : `<li class="meta">${t("Keine Unterordner.")}</li>`;
  } catch (e) {
    ul.innerHTML = `<li class="meta">${esc(e)}</li>`;
  }
}

$("#folder-list").addEventListener("click", (ev) => {
  const li = ev.target.closest("li[data-name]");
  if (!li) return;
  const base = $("#folder-path").value.replace(/\/+$/, "");
  browseFolder(`${base}/${li.dataset.name}`);
});
$("#folder-crumbs").addEventListener("click", (ev) => {
  const b = ev.target.closest("button[data-path]");
  if (b) browseFolder(b.dataset.path);
});
$("#folder-add").addEventListener("click", async () => {
  const path = $("#folder-path").value.trim();
  if (!path || path === "/") return showError(t("Bitte einen Ordner auf einem Laufwerk wählen, z. B. /USB0/Spiele."));
  try {
    const id = await invoke("add_device_folder", { deviceId: folderDevice.id, path, quick: $("#folder-quick").checked });
    $("#folder-dialog").close();
    await loadSources();
    await startScan([id]);
  } catch (e) {
    showError(e);
  }
});

const DISK_FORMATS = ["D64", "D71", "D81", "G64"];

// Diskettenreihen eines zusammengefassten Spiels: gleiche Reihe, verschiedene Seiten, sortiert.
function diskSets(variants) {
  const sets = new Map();
  for (const v of variants) {
    if (!v.disk_set || !v.disk) continue;
    if (!sets.has(v.disk_set)) sets.set(v.disk_set, []);
    sets.get(v.disk_set).push(v);
  }
  return [...sets.values()].filter((l) => l.length > 1)
    .map((l) => l.sort((a, b) => a.disk.localeCompare(b.disk, undefined, { numeric: true })));
}

function disksHtml(variants) {
  const sets = diskSets(variants);
  if (!sets.length) return "";
  const canRun = devices.length > 0;
  return `<h4>${t("Disketten")}</h4>${sets.map((set) => `
    <ol class="disk-set">${set.map((v, i) => `
      <li>
        <span class="disk-no">${esc(/^[A-Z]$/.test(v.disk) && !v.disk_set.startsWith("dir:") ? t("Seite {s}", { s: v.disk }) : t("Disk {s}", { s: v.disk }))}</span>
        <span class="v-title" title="${esc(v.title)}">${esc(v.title)} <span class="badge">${esc(v.format)}</span></span>
        ${canRun && i === 0 ? `<button class="small primary" data-start="${v.sha1}" title="${t("Ab dieser Disk starten – beendet, was gerade auf dem C64 läuft")}">${t("Starten")}</button>` : ""}
        ${canRun ? `<button class="small" data-insert="${v.sha1}" title="${t("In Laufwerk A einlegen, ohne Neustart – für den Diskwechsel im laufenden Spiel")}">${t("Einlegen")}</button>` : ""}
      </li>`).join("")}</ol>`).join("")}`;
}

// Bildschirmfotos über den Videostrom können Ultimate 64 und C64 Ultimate, nicht das 1541 Ultimate II+.
const canShoot = (d) => !!d && !/1541|II\+/i.test(d.product || "");

function launchHtml(r) {
  if (!devices.length) return "";
  const sel = storedDeviceId();
  const disk = DISK_FORMATS.includes(r.format);
  const tap = r.format === "TAP";
  const shot = devices.some(canShoot)
    ? `<button id="btn-shot" title="${t("Bild vom Bildschirm des Geräts als eigenes Titelbild oder eigene Spielszene speichern – nur mit Netzwerkkabel am Gerät")}">${t("Bildschirmfoto")}</button>` : "";
  return `<div class="launch">
    <select id="launch-device">${devices.map((d) =>
      `<option value="${d.id}" ${d.id === sel ? "selected" : ""}>${esc(d.name)}</option>`).join("")}</select>
    ${tap ? "" : `<button class="primary" id="btn-launch">${t("Starten")}</button>`}
    ${disk ? `<button id="btn-insert" title="${t("In Laufwerk A einlegen, ohne Neustart – für den Diskwechsel im laufenden Spiel")}">${t("Einlegen")}</button>` : ""}
    ${disk ? `<button id="btn-saves" title="${t("Hat das Spiel auf seine Diskette im Startordner gespeichert, diese Diskette mit Spielstand auf den Rechner holen")}">${t("Spielstand holen …")}</button>` : ""}
    ${shot}
    <span class="meta hint">${tap ? t("Bandabbilder (TAP) lassen sich noch nicht starten.")
      : disk ? t("Starten beendet, was gerade auf dem C64 läuft, und startet dieses Spiel; Einlegen wechselt nur die Diskette in Laufwerk A.")
      : t("Starten beendet, was gerade auf dem C64 läuft, und startet dieses Spiel.")}</span>
  </div>`;
}

async function captureScreen(pane, r, btn) {
  const deviceId = Number(pane.querySelector("#launch-device").value);
  const dev = devices.find((d) => d.id === deviceId);
  if (!canShoot(dev)) {
    showError(t("{name} liefert kein Bild – Bildschirmfotos gehen mit Ultimate 64 und C64 Ultimate.", { name: dev?.name ?? "?" }));
    return;
  }
  const title = t("Titelbild"), scene = t("Spielszene");
  const answer = await invoke("plugin:dialog|message", {
    message: t("Was gerade auf {name} zu sehen ist, als eigenes Bild speichern – als was?", { name: dev.name })
      + "\n\n" + t("Die Ultimate schickt ihr Bild dafür kurz als Videostrom – nur über Ethernet, das Gerät muss per Netzwerkkabel verbunden sein. Andere Empfänger (etwa eine Jukebox) verlieren den Strom dabei."),
    title: t("Bildschirmfoto"), kind: "info",
    buttons: { YesNoCancelCustom: [title, scene, t("Abbrechen")] },
  });
  const kind = answer === "Yes" || answer === title || answer?.Custom === title ? "sstitle"
    : answer === "No" || answer === scene || answer?.Custom === scene ? "ss" : null;
  if (!kind) return;
  btn.disabled = true;
  setStatus(t("Nehme das Bild von {name} auf …", { name: dev.name }));
  try {
    await invoke("capture_screen", { deviceId, sha1s: [r.sha1], kind, crop: true });
    setStatus(t("Bildschirmfoto gespeichert."));
    await showDetails(r.sha1);
  } catch (e) {
    showError(e);
  } finally {
    btn.disabled = false;
  }
}

function wireLaunch(pane, r) {
  pane.querySelector("#btn-shot")?.addEventListener("click", (ev) => captureScreen(pane, r, ev.currentTarget).catch(showError));
  pane.querySelector("#btn-saves")?.addEventListener("click", () => {
    const dev = devices.find((d) => d.id === Number(pane.querySelector("#launch-device").value));
    const sha1s = [...new Set([r.sha1, ...lastVariants.map((v) => v.sha1), ...currentGroup])];
    if (dev) fetchSaves(dev, sha1s).catch(showError);
  });
  const btn = pane.querySelector("#btn-launch");
  if (!btn) return;
  btn.addEventListener("click", () => launchOn(pane, r.sha1, r.title, false, btn));
  pane.querySelector("#btn-insert")?.addEventListener("click", (ev) => launchOn(pane, r.sha1, r.title, true, ev.currentTarget));
  // Andere Disk/Seite desselben Spiels einlegen oder starten, ohne die Details zu wechseln.
  pane.querySelectorAll("button[data-insert], button[data-start]").forEach((b) => b.addEventListener("click", () => {
    const target = b.dataset.insert ?? b.dataset.start;
    const v = lastVariants.find((x) => x.sha1 === target);
    launchOn(pane, target, v?.title ?? "", !!b.dataset.insert, b);
  }));
}

// Automatisches Titelbild: nach dem Start warten und fotografieren, wenn das Spiel noch keines hat.
// Ein weiterer Start auf demselben Gerät ersetzt die wartende Aufnahme (Einlegen nicht).
const autoTitleTimers = new Map();

async function autoTitle(dev, sha1) {
  clearTimeout(autoTitleTimers.get(dev.id));
  autoTitleTimers.delete(dev.id);
  if (!scraperSettings.auto_title || !canShoot(dev)) return;
  const d = await invoke("details", { sha1 });
  if (d.media.some(([kind]) => kind === "sstitle")) return;
  const secs = scraperSettings.auto_title_delay ?? 25;
  autoTitleTimers.set(dev.id, setTimeout(async () => {
    autoTitleTimers.delete(dev.id);
    try {
      // Inzwischen von Hand ein Titelbild gesetzt? Dann nichts überschreiben.
      const now = await invoke("details", { sha1 });
      if (now.media.some(([kind]) => kind === "sstitle")) return;
      await invoke("capture_screen", { deviceId: dev.id, sha1s: [sha1], kind: "sstitle", crop: true });
      setStatus(t("Titelbild von {name} aufgenommen – in den Details lässt es sich ersetzen.", { name: dev.name }));
      if (selectedSha1 === sha1) await showDetails(sha1);
    } catch (e) {
      setStatus(t("Kein automatisches Titelbild: {e}", { e: String(e) }));
    }
  }, secs * 1000));
}

async function launchOn(pane, sha1, title, insertOnly, btn) {
  const deviceId = Number(pane.querySelector("#launch-device").value);
  try { localStorage.setItem("launchDevice", String(deviceId)); } catch { /* egal */ }
  const dev = devices.find((d) => d.id === deviceId);
  btn.disabled = true;
  try {
    // Gerät aus? Dann erst einschalten, sofern das geht.
    const st = deviceStatus[deviceId] ?? await refreshDeviceStatus(deviceId);
    if (st && !st.online) {
      const now = await refreshDeviceStatus(deviceId);
      if (now && !now.online) {
        if (!now.can_wake) throw t("{name} ist aus und lässt sich nicht aus der App einschalten.", { name: dev.name });
        await wakeDevice(dev);
      }
    }
    setStatus(insertOnly
      ? t("Lege „{title}“ in {name} ein …", { title, name: dev.name })
      : t("Starte „{title}“ auf {name} …", { title, name: dev.name }));
    const res = await invoke("launch_game", { sha1, deviceId, insertOnly });
    setStatus(`${dev.name}: ${res.message}${res.copied ? t(" (vorher hinkopiert)") : ""}`);
    if (!insertOnly) autoTitle(dev, sha1).catch(() => { /* nur ein Zusatz */ });
    if ($("#view").value === "recent") runSearch().catch(showError);
  } catch (e) {
    showError(e);
  } finally {
    btn.disabled = false;
  }
}

// ---------------------------------------------------------------- Breiten

// Seitenleiste und Details lassen sich per Ziehgriff verbreitern; die Breiten bleiben gemerkt.
// Fenstergröße und -lage merkt sich die App selbst (Tauri-Plugin window-state).
const PANES = {
  sidebar: { handle: "#split-sidebar", prop: "--sidebar-w", min: 200, max: 560, fromLeft: true },
  details: { handle: "#split-details", prop: "--details-w", min: 300, max: 900, fromLeft: false },
};

function setPaneWidth(pane, px) {
  const p = PANES[pane];
  const w = Math.round(Math.min(p.max, Math.max(p.min, px)));
  document.documentElement.style.setProperty(p.prop, `${w}px`);
  return w;
}

for (const [pane, p] of Object.entries(PANES)) {
  try {
    const saved = Number(localStorage.getItem(`width.${pane}`));
    if (saved) setPaneWidth(pane, saved);
  } catch { /* egal */ }
  const handle = $(p.handle);
  handle.addEventListener("pointerdown", (ev) => {
    ev.preventDefault();
    handle.setPointerCapture(ev.pointerId);
    handle.classList.add("dragging");
    document.body.classList.add("resizing");
    let width = null;
    const move = (e) => { width = setPaneWidth(pane, p.fromLeft ? e.clientX : window.innerWidth - e.clientX); };
    const up = () => {
      handle.removeEventListener("pointermove", move);
      handle.classList.remove("dragging");
      document.body.classList.remove("resizing");
      if (width) try { localStorage.setItem(`width.${pane}`, String(width)); } catch { /* egal */ }
    };
    handle.addEventListener("pointermove", move);
    handle.addEventListener("pointerup", up, { once: true });
    handle.addEventListener("pointercancel", up, { once: true });
  });
  handle.addEventListener("dblclick", () => {
    document.documentElement.style.removeProperty(p.prop);
    try { localStorage.removeItem(`width.${pane}`); } catch { /* egal */ }
  });
}

// Der Griff vor den Details erscheint nur, wenn die Details zu sehen sind.
new MutationObserver(() => { $("#split-details").hidden = $("#details").hidden; })
  .observe($("#details"), { attributes: true, attributeFilter: ["hidden"] });

// ---------------------------------------------------------------- Aufräumen

async function openCleanup() {
  const c = await invoke("cleanup");
  const block = (title, text, buttons) => `
    <div class="cleanup-block">
      <h4>${title}</h4>
      <p>${text}</p>
      <div class="meta-actions">${buttons}</div>
    </div>`;
  $("#cleanup-body").innerHTML = [
    block(t("Doppelte Dateien"),
      c.dup_contents
        ? t("{n} Inhalte liegen an mehreren Orten ({files} Dateien); die zusätzlichen Kopien belegen {size}.", { n: nf(c.dup_contents), files: nf(c.dup_files), size: fmtSize(c.dup_bytes) })
          + (c.same_source_contents ? " " + t("Davon {n} mehrfach in derselben Quelle ({size}) – Kopien auf anderen Datenträgern wie einem Gerätestick sind oft gewollt.", { n: nf(c.same_source_contents), size: fmtSize(c.same_source_bytes) }) : "")
        : t("Keine doppelten Dateien."),
      c.dup_contents ? `<button type="button" data-clean="dupes">${t("Zeigen")}</button><button type="button" data-clean="dupes-csv">${t("Liste mit allen Orten speichern …")}</button>` : ""),
    block(t("Beschädigt oder unbekannt aufgebaut"),
      c.damaged ? t("{n} Abbilder ließen sich beim Scan nicht lesen – oft unvollständige Downloads oder seltene Sonderformate.", { n: nf(c.damaged) }) : t("Keine."),
      c.damaged ? `<button type="button" data-clean="damaged">${t("Zeigen")}</button>` : ""),
    block(t("Ohne Angaben"),
      c.without_meta ? t("{n} Einträge haben keine Beschreibung: nicht erkannt oder noch nicht nachgeschlagen. Hier helfen Gemeinschaftsdaten, ScreenScraper oder eigene Angaben.", { n: nf(c.without_meta) }) : t("Alle Einträge haben Angaben."),
      c.without_meta ? `<button type="button" data-clean="nometa">${t("Zeigen")}</button>` : ""),
  ].join("");
  $("#cleanup-body").querySelectorAll("[data-clean]").forEach((b) => b.addEventListener("click", async () => {
    const act = b.dataset.clean;
    if (act === "dupes-csv") {
      try {
        const path = await invoke("plugin:dialog|save", { options: { title: t("Doppelte Dateien speichern"), defaultPath: "breadbin-doppelt.csv", filters: [{ name: "CSV", extensions: ["csv"] }] } });
        if (!path) return;
        const n = await invoke("export_csv", { options: { only_duplicates: true, limit: 0, group: false }, path });
        setStatus(t("{n} Einträge nach {path} exportiert.", { n: nf(n), path }));
      } catch (e) { showError(e); }
      return;
    }
    $("#cleanup-dialog").close();
    $("#search").value = "";
    $("#only-dupes").checked = act === "dupes";
    $("#only-meta").checked = false;
    $("#view").value = act === "dupes" ? "" : act;
    runSearch().catch(showError);
  }));
  $("#cleanup-dialog").showModal();
}

$("#btn-cleanup").addEventListener("click", () => openCleanup().catch(showError));

// ---------------------------------------------------------------- Auf Gerät kopieren, Spielstände

async function openCopyDialog(rows) {
  const sel = $("#copy-device");
  const stored = storedDeviceId();
  sel.innerHTML = devices.map((d) => `<option value="${d.id}" ${d.id === stored ? "selected" : ""}>${esc(d.name)}</option>`).join("");
  const listName = $("#view").value.startsWith("list:")
    ? gameLists.find((l) => `list:${l.id}` === $("#view").value)?.name : null;
  const suggest = () => {
    const dev = devices.find((d) => d.id === Number(sel.value));
    const sub = (listName ?? t("Auswahl")).replace(/[\\/:*?"<>|]/g, "_");
    $("#copy-target").value = `/${(dev?.upload_dir ?? "/USB0/Breadbin").replace(/^\/+|\/+$/g, "")}/${sub}`;
  };
  sel.onchange = suggest;
  suggest();
  $("#copy-info").textContent = t("{n} Spiele kopieren. Was schon auf dem Gerät liegt, wird nicht noch einmal kopiert.", { n: rows.length });
  $("#copy-dialog").returnValue = "";
  $("#copy-dialog").showModal();
  $("#copy-dialog").onclose = async () => {
    if ($("#copy-dialog").returnValue !== "ok") return;
    const dev = devices.find((d) => d.id === Number(sel.value));
    setStatus(t("Kopiere {n} Spiele auf {name} …", { n: rows.length, name: dev.name }));
    try {
      const r = await invoke("copy_to_device", { deviceId: dev.id, sha1s: rows.map((x) => x.sha1), target: $("#copy-target").value });
      const lines = [t("{n} kopiert, {m} lagen schon auf dem Gerät.", { n: r.copied.length, m: r.present.length })];
      if (r.present.length) lines.push("", t("Schon vorhanden:"), ...r.present.slice(0, 15), ...(r.present.length > 15 ? ["…"] : []));
      if (r.failed.length) lines.push("", t("Nicht kopiert:"), ...r.failed.slice(0, 15));
      setStatus(lines[0]);
      await invoke("plugin:dialog|message", { message: lines.join("\n"), title: t("Auf Gerät kopieren"), kind: r.failed.length ? "warning" : "info" });
    } catch (e) { showError(e); }
  };
}

// sha1s: nur die Disketten eines Spiels (Knopf in den Details); ohne: alle geänderten im Startordner.
async function fetchSaves(dev, sha1s = null) {
  let start = null;
  try { start = localStorage.getItem("savesDir"); } catch { /* egal */ }
  const dir = await invoke("plugin:dialog|open", { options: { directory: true, title: t("Ordner für Spielstände wählen"), defaultPath: start ?? undefined } });
  if (!dir) return;
  try { localStorage.setItem("savesDir", dir); } catch { /* egal */ }
  setStatus(t("Suche geänderte Disketten auf {name} …", { name: dev.name }));
  const files = await invoke("fetch_saves", { deviceId: dev.id, target: dir, sha1s });
  const msg = files.length
    ? t("{n} Disketten mit Spielstand geholt:", { n: files.length }) + "\n\n" + files.map((f) => f.split(/[\\/]/).pop()).join("\n")
      + "\n\n" + t("Ist der Ordner eine Quelle, erscheinen sie nach dem nächsten Scan als eigene Fassung.")
    : sha1s ? t("Kein neuer Spielstand zu diesem Spiel im Startordner von {name} (oder schon geholt).", { name: dev.name })
      + "\n\n" + t("Alle Spielstände eines Geräts: ⋯ in der Gerätezeile → „Spielstände zurückholen …“.")
    : t("Keine geänderten Disketten im Startordner (oder schon geholt).");
  setStatus(files.length ? t("{n} Spielstände geholt.", { n: files.length }) : t("Keine neuen Spielstände."));
  await invoke("plugin:dialog|message", { message: msg, title: t("Spielstände zurückholen"), kind: "info" });
}

// ---------------------------------------------------------------- Eigene Listen und Notizen

async function loadLists() {
  gameLists = await invoke("lists");
  const sel = $("#view");
  const cur = sel.value;
  sel.querySelector("optgroup[data-lists]")?.remove();
  const g = document.createElement("optgroup");
  g.label = t("Eigene Listen");
  g.dataset.lists = "1";
  g.innerHTML = gameLists.map((l) => `<option value="list:${l.id}">${esc(l.name)} (${nf(l.count)})</option>`).join("")
    + `<option value="manage-lists">${t(gameLists.length ? "Listen verwalten …" : "Liste anlegen …")}</option>`;
  sel.appendChild(g);
  sel.value = [...sel.options].some((o) => o.value === cur && cur !== "manage-lists") ? cur : "";
  viewBefore = sel.value;
  if (sel.value !== cur) runSearch().catch(showError);
}

// „Für mich“ in den Details: Bewertung, durchgespielt, Notiz und Listen – für alle Fassungen der Zeile.
async function renderMine(box, sha1s) {
  if (!box) return;
  const { note, lists } = await invoke("game_note", { sha1s });
  if (selectedSha1 !== sha1s[0]) return;
  const cur = { ...note };
  box.innerHTML = `
    <h4>${t("Für mich")}</h4>
    <div class="mine-row">
      <span class="stars" role="group" aria-label="${t("Eigene Bewertung")}">${[1, 2, 3, 4, 5].map((n) =>
        `<button type="button" class="star-rate ${n <= cur.rating ? "on" : ""}" data-rate="${n}" title="${t("{n} von 5", { n })}">★</button>`).join("")}</span>
      <label class="check"><input type="checkbox" data-done ${cur.done ? "checked" : ""}> ${t("Durchgespielt")}</label>
      <span class="meta" data-saved></span>
    </div>
    <textarea data-note rows="3" placeholder="${t("Notizen, Codes, Tipps …")}">${esc(cur.note)}</textarea>
    <div class="mine-lists">
      ${gameLists.map((l) => `<label class="chip"><input type="checkbox" data-list="${l.id}" ${lists.includes(l.id) ? "checked" : ""}> ${esc(l.name)}</label>`).join("")}
      <span class="new-list"><input data-newlist placeholder="${t("Neue Liste …")}"><button type="button" class="small" data-addlist>${t("Anlegen")}</button></span>
      ${gameLists.length ? `<button type="button" class="link" data-manage>${t("Listen verwalten …")}</button>` : ""}
    </div>`;
  const saved = box.querySelector("[data-saved]");
  let timer = null;
  const save = async (refresh) => {
    clearTimeout(timer);
    await invoke("save_note", { sha1s, note: cur });
    saved.textContent = t("gespeichert");
    if (refresh) runSearch().catch(showError);
  };
  box.querySelectorAll("[data-rate]").forEach((b) => b.addEventListener("click", () => {
    const n = Number(b.dataset.rate);
    cur.rating = n === cur.rating ? 0 : n;
    box.querySelectorAll("[data-rate]").forEach((x) => x.classList.toggle("on", Number(x.dataset.rate) <= cur.rating));
    save(true).catch(showError);
  }));
  box.querySelector("[data-done]").addEventListener("change", (ev) => {
    cur.done = ev.target.checked;
    save(true).catch(showError);
  });
  const ta = box.querySelector("[data-note]");
  ta.addEventListener("input", () => {
    cur.note = ta.value;
    saved.textContent = "";
    clearTimeout(timer);
    timer = setTimeout(() => save(false).catch(showError), 700);
  });
  ta.addEventListener("blur", () => { if (timer) save(false).catch(showError); });
  box.querySelectorAll("[data-list]").forEach((c) => c.addEventListener("change", async () => {
    try {
      await invoke("set_in_list", { id: Number(c.dataset.list), sha1s, on: c.checked });
      await loadLists();
      if ($("#view").value === `list:${c.dataset.list}`) runSearch().catch(showError);
    } catch (e) { showError(e); }
  }));
  const addList = async () => {
    const input = box.querySelector("[data-newlist]");
    const id = await invoke("create_list", { name: input.value });
    await invoke("set_in_list", { id, sha1s, on: true });
    await loadLists();
    await renderMine(box, sha1s);
  };
  box.querySelector("[data-addlist]").addEventListener("click", () => addList().catch(showError));
  box.querySelector("[data-newlist]").addEventListener("keydown", (ev) => {
    if (ev.key === "Enter") { ev.preventDefault(); addList().catch(showError); }
  });
  box.querySelector("[data-manage]")?.addEventListener("click", () => openListsDialog().catch(showError));
}

async function openListsDialog() {
  await loadLists();
  const body = $("#lists-body");
  const render = () => {
    body.innerHTML = gameLists.length ? gameLists.map((l) => `
      <div class="list-row" data-id="${l.id}">
        <input value="${esc(l.name)}" data-name aria-label="${t("Name")}">
        <span class="meta">${t("{n} Fassungen", { n: nf(l.count) })}</span>
        <button type="button" class="small danger" data-del>${t("Löschen")}</button>
      </div>`).join("") : `<p class="meta">${t("Noch keine Listen.")}</p>`;
    body.querySelectorAll(".list-row").forEach((row) => {
      const id = Number(row.dataset.id);
      row.querySelector("[data-name]").addEventListener("change", async (ev) => {
        try { await invoke("rename_list", { id, name: ev.target.value }); await loadLists(); } catch (e) { showError(e); }
      });
      row.querySelector("[data-del]").addEventListener("click", async () => {
        const l = gameLists.find((x) => x.id === id);
        const answer = await invoke("plugin:dialog|message", {
          message: t("Liste „{name}“ löschen? Die {n} Fassungen darin bleiben in der Bibliothek.", { name: l?.name ?? "", n: nf(l?.count ?? 0) }),
          title: t("Eigene Listen"), kind: "warning",
          buttons: { OkCancelCustom: [t("Löschen"), t("Abbrechen")] },
        }).catch(() => null);
        if (!confirmed(answer, t("Löschen"))) return;
        try { await invoke("delete_list", { id }); await loadLists(); render(); } catch (e) { showError(e); }
      });
    });
  };
  const add = async () => {
    const input = $("#lists-new");
    if (!input.value.trim()) return;
    await invoke("create_list", { name: input.value });
    input.value = "";
    await loadLists();
    render();
  };
  $("#lists-add").onclick = () => add().catch(showError);
  $("#lists-new").onkeydown = (ev) => { if (ev.key === "Enter") { ev.preventDefault(); add().catch(showError); } };
  render();
  $("#lists-dialog").showModal();
}

$("#lists-close").addEventListener("click", async () => {
  $("#lists-dialog").close();
  if (selectedSha1 && !$("#details").hidden && marked.size < 2) await showDetails(selectedSha1).catch(showError);
});

// ---------------------------------------------------------------- Neue Fassung, Gemeinschaftsdaten

let updateInfo = null;

function showUpdate(info) {
  updateInfo = info;
  const btn = $("#btn-update");
  btn.hidden = !info;
  if (info) {
    btn.textContent = t("Breadbin {v} ist da", { v: info.version });
    btn.title = (LANG === "de" ? info.notes_de : info.notes_en || info.notes_de) || t("Zum Herunterladen");
  }
}

$("#btn-update").addEventListener("click", () => {
  if (updateInfo?.url) invoke("plugin:opener|open_url", { url: updateInfo.url }).catch(showError);
});

// Beides im Hintergrund nach dem Start; Fehler (etwa ohne Netz) nur in der Konsole, nicht als Meldung.
async function backgroundChecks() {
  try {
    showUpdate(await invoke("check_update", { force: false }));
  } catch (e) {
    console.warn("Versionsprüfung:", e);
  }
  try {
    const r = await invoke("community_auto_update");
    if (r && r.imported) {
      setStatus(t("Gemeinschaftsdaten selbst aktualisiert: {n} übernommen.", { n: nf(r.imported) }));
      await loadStats();
      await runSearch();
    }
  } catch (e) {
    console.warn("Gemeinschaftsdaten:", e);
  }
}

// ---------------------------------------------------------------- Start

(async () => {
  translateDom();
  try {
    await invoke("set_language", { lang: LANG });
    await loadSources();
    await loadDevices();
    await loadScraperSettings();
    await loadLists();
    await runSearch();
  } catch (e) {
    showError(e);
  }
  backgroundChecks();
})();
