#!/usr/bin/env python3
"""Baut website/anleitung/index.html aus dem Handbuch der App (ui/help.de.html, ui/help.en.html).

So gibt es nur einen Text: Was in der App unter „?“ / F1 steht, steht auch auf der Webseite.
Builds the website manual page from the app's built-in help, so there is only one text to maintain.
Aufruf / usage: python3 .github/anleitung.py   (läuft im Workflow „Webseite und Downloads“ vor dem Hochladen)
"""
import pathlib
import re

ROOT = pathlib.Path(__file__).resolve().parents[1]
OUT = ROOT / "website" / "anleitung" / "index.html"


def body(lang: str) -> str:
    html = (ROOT / "ui" / f"help.{lang}.html").read_text(encoding="utf-8")
    # Anker je Sprache eindeutig machen (beide Fassungen stehen auf einer Seite)
    html = re.sub(r'id="(h-[\w-]+)"', rf'id="{lang}-\1"', html)
    html = re.sub(r'href="#(h-[\w-]+)"', rf'href="#{lang}-\1"', html)
    return html


PAGE = """<!doctype html>
<html lang="de">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width,initial-scale=1">
<title>Breadbin – Anleitung / Manual</title>
<meta name="description" content="Anleitung zu Breadbin, der Spielebibliothek für den C64 – dieselbe Hilfe wie in der App. Manual for Breadbin, the C64 game library.">
<link rel="icon" href="../bilder/icon.png">
<style>
  /* Gleiche Farbwelt wie die Startseite (Brotkasten-Beige / C64-Blau). Inhalt: ui/help.*.html, erzeugt von .github/anleitung.py */
  :root {
    --bg: #ece6d6; --panel: #f7f3e9; --line: #d5ccb5; --fg: #2b2620; --muted: #6d6455;
    --accent: #40318d; --accent-fg: #fff; --code: #e2dac6; color-scheme: light;
  }
  @media (prefers-color-scheme: dark) {
    :root { --bg: #1f1a4e; --panel: #2c2575; --line: #3e3596; --fg: #d4ceff; --muted: #a59ee6;
      --accent: #a99fff; --accent-fg: #1f1a4e; --code: #262060; color-scheme: dark; }
  }
  * { box-sizing: border-box; }
  html { scroll-behavior: smooth; }
  @media (prefers-reduced-motion: reduce) { html { scroll-behavior: auto; } }
  body { margin: 0; background: var(--bg); color: var(--fg); font: 17px/1.6 system-ui, -apple-system, "Segoe UI", Roboto, sans-serif; }
  .wrap { max-width: 860px; margin: 0 auto; padding-left: max(16px, env(safe-area-inset-left)); padding-right: max(16px, env(safe-area-inset-right)); }
  .stripe { height: 4px; background: linear-gradient(90deg, #c0392b, #e67e22, #f1c40f, #27ae60, #2c6fbb); }
  header.top { position: sticky; top: 0; z-index: 10; backdrop-filter: blur(8px); background: color-mix(in srgb, var(--bg) 85%, transparent); border-bottom: 1px solid var(--line); }
  header.top .wrap { display: flex; align-items: center; justify-content: space-between; gap: 12px; padding-block: 10px; flex-wrap: wrap; max-width: 1120px; }
  .brand { display: flex; gap: 10px; align-items: center; font: 700 19px ui-monospace, Menlo, monospace; color: var(--accent); text-decoration: none; }
  .brand img { width: 30px; height: 30px; }
  nav { display: flex; gap: 18px; align-items: center; flex-wrap: wrap; font-size: 15px; }
  nav a { color: var(--fg); text-decoration: none; } nav a:hover { color: var(--accent); }
  .lang { background: none; border: 1px solid var(--line); color: var(--fg); border-radius: 6px; padding: 3px 10px; font: inherit; font-size: 14px; cursor: pointer; }
  h1 { font: 700 clamp(30px, 5vw, 44px)/1.1 ui-monospace, "SF Mono", Menlo, Consolas, monospace; margin: 40px 0 8px; letter-spacing: -1px; text-wrap: balance; }
  .lede { color: var(--muted); margin: 0 0 8px; max-width: 62ch; }
  .help { padding-bottom: 64px; }
  .help h3 { font: 700 21px/1.3 ui-monospace, "SF Mono", Menlo, Consolas, monospace; color: var(--accent); margin: 40px 0 8px; scroll-margin-top: 72px; text-wrap: balance; }
  .help p, .help li { max-width: 72ch; }
  .help ul, .help ol { padding-left: 22px; }
  .help li { margin: 5px 0; }
  .help a { color: var(--accent); }
  .help code { font-size: .88em; background: var(--code); padding: 1px 5px; border-radius: 4px; overflow-wrap: anywhere; }
  .help kbd { font: 600 .85em ui-monospace, Menlo, monospace; border: 1px solid var(--line); border-bottom-width: 2px; border-radius: 4px; padding: 0 5px; background: var(--panel); }
  .help-toc { display: flex; flex-wrap: wrap; gap: 8px; padding: 14px 0 6px; }
  .help-toc a { font-size: 14px; padding: 3px 11px; border: 1px solid var(--line); border-radius: 999px; text-decoration: none; background: var(--panel); }
  .help-toc a:hover { border-color: var(--accent); }
  .help table { border-collapse: collapse; display: block; overflow-x: auto; max-width: 100%; }
  .help td, .help th { padding: 4px 10px; border-bottom: 1px solid var(--line); text-align: left; }
  footer { border-top: 1px solid var(--line); color: var(--muted); font-size: 14px; padding-block: 18px 32px; }
  footer a { color: var(--accent); }
  [hidden] { display: none !important; }
</style>
</head>
<body>
<div class="stripe"></div>
<header class="top"><div class="wrap">
  <a class="brand" href="../"><img src="../bilder/icon.png" alt="">Breadbin</a>
  <nav>
    <a href="../" data-de="Startseite" data-en="Home">Startseite</a>
    <a href="../#download">Download</a>
    <a href="https://breadbindb.8bitcavern.de/" data-de="Gemeinschaftsdaten" data-en="Community data">Gemeinschaftsdaten</a>
    <button class="lang" id="lang" type="button">English</button>
  </nav>
</div></header>
<main class="wrap">
  <section lang="de" data-sprache="de">
    <h1>Anleitung</h1>
    <p class="lede">Dieselbe Hilfe wie in der App (Knopf „?“ oder F1) – zum Nachlesen schon vor dem Download.</p>
    <div class="help">
@@DE@@
    </div>
  </section>
  <section lang="en" data-sprache="en" hidden>
    <h1>Manual</h1>
    <p class="lede">The same help as in the app (“?” button or F1) – to read even before downloading.</p>
    <div class="help">
@@EN@@
    </div>
  </section>
</main>
<footer><div class="wrap">
  <span data-de="Ein Projekt von" data-en="A project by">Ein Projekt von</span> <a href="https://8bitcavern.de">8bitcavern.de</a> ·
  <a href="https://github.com/sedorrr/breadbin">GitHub</a> · breadbin@8bitcavern.de
</div></footer>
<script>
let lang = (navigator.language || "de").slice(0, 2) === "de" ? "de" : "en";
try { lang = localStorage.getItem("lang") || lang; } catch { /* egal */ }
function setLang(l) {
  lang = l;
  document.documentElement.lang = l;
  document.querySelectorAll("[data-sprache]").forEach((s) => { s.hidden = s.dataset.sprache !== l; });
  document.querySelectorAll("[data-de]").forEach((e) => { e.textContent = e.dataset[l]; });
  document.getElementById("lang").textContent = l === "de" ? "English" : "Deutsch";
  try { localStorage.setItem("lang", l); } catch { /* egal */ }
}
document.getElementById("lang").addEventListener("click", () => setLang(lang === "de" ? "en" : "de"));
// Anker aus der anderen Sprache (z. B. #h-share) auf die passende Fassung lenken
const h = location.hash.slice(1);
if (/^(de|en)-/.test(h)) lang = h.slice(0, 2);
setLang(lang);
if (/^h-/.test(h)) document.getElementById(lang + "-" + h)?.scrollIntoView();
</script>
</body>
</html>
"""


def main() -> None:
    OUT.parent.mkdir(parents=True, exist_ok=True)
    OUT.write_text(PAGE.replace("@@DE@@", body("de")).replace("@@EN@@", body("en")), encoding="utf-8")
    print(f"{OUT.relative_to(ROOT)} geschrieben")


if __name__ == "__main__":
    main()
