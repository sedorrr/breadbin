#!/usr/bin/env python3
"""Erzeugt die Bildschirmfotos für die Anleitung auf der Webseite (website/bilder/anleitung/*-de.jpg / *-en.jpg).

Aus der aktuellen Oberfläche (ui/index.html) entsteht eine Nachbildung mit Beispieldaten (nachgebildetes
`window.__TAURI__`), davon macht Playwright (Chromium) Aufnahmen einzelner Bereiche. Lokal aufrufen, wenn sich
die Oberfläche sichtbar geändert hat; die Bilder werden mit committet.

    python3 .github/anleitung_bilder.py <ordner-mit-beispieldaten>

Der Datenordner enthält rows.json, details.json und media/ (Bilder) – Ausschnitt einer echten Bibliothek, nicht im
Repository. Generates the screenshots for the website manual from a mock of the current UI.
"""
import http.server

import pathlib
import shutil
import sys
import tempfile
import threading

from playwright.sync_api import sync_playwright

ROOT = pathlib.Path(__file__).resolve().parents[1]
OUT = ROOT / "website" / "bilder" / "anleitung"
SEL = "9bf24d2da79366ea91e51f5f758f52d39a4cb746"  # Beispielspiel für die Details

MOCK = r"""<script>
window.__calls=[];
const ROWS=[], DET={};
const xhr=(u)=>{const x=new XMLHttpRequest();x.open("GET",u,false);x.send();return JSON.parse(x.responseText);};
ROWS.push(...xhr("data/rows.json")); Object.assign(DET, xhr("data/details.json"));
const LANGQ=new URLSearchParams(location.search).get("lang"); if(LANGQ){try{localStorage.setItem("uiLang",LANGQ)}catch{}}
const DEVS=[
 {id:7,name:"Ultimate 64 Elite II",host:"192.0.2.10",product:"Ultimate 64 Elite II",upload_dir:"/USB2/Breadbin",kind:"ultimate",wifi_mac:"",unique_id:"",mac:""},
 {id:8,name:"C64 Ultimate",host:"192.0.2.11",product:"C64 Ultimate",upload_dir:"/USB0/Breadbin",kind:"ultimate",wifi_mac:"x",unique_id:"",mac:""},
 {id:9,name:"BMX (Pi 4)",host:"192.0.2.12",product:"BMX (Raspberry Pi 4 Model B)",upload_dir:"/USER/disks/Breadbin",kind:"bmx",wifi_mac:"",unique_id:"",mac:"x"}];
const R={
 emu_options:()=>"", save_emu_options:()=>null, set_language:()=>null,
 get_scraper_settings:()=>({user:"",has_password:false,dev_builtin:true,dev_id:"",has_dev_password:false,language:"de",name_fallback:true,threads:0,keep_awake:true,preferred_source:"community",community_url:"https://breadbindb.8bitcavern.de/breadbin-db.zip",default_community_url:"https://breadbindb.8bitcavern.de/breadbin-db.zip",submit_url:"x",default_submit_url:"x",update_check:true,community_auto:true,auto_title:true,auto_title_delay:25,emulators:[{path:"/Applications/vice-arm64-sdl2-3.9/x64sc.app",args:""}],softname:"Breadbin"}),
 list_devices:()=>DEVS,
 device_status:(a)=>a.id===8?{online:false,can_wake:true,can_power_off:true,firmware:"",kernal:"",hints:[]}:{online:true,can_wake:a.id!==9,can_power_off:a.id!==9,firmware:a.id===9?"2026.11.22":"3.15a",kernal:"",hints:[]},
 list_sources:()=>[{id:1,kind:"local",label:"C64-Sammlung",location:"~/C64",last_scan:1795700000,files:43859,quick:false},{id:3,kind:"ultimate",label:"Ultimate 64 Elite II: /USB2",location:"ftp://192.0.2.10/USB2",last_scan:1795700000,files:2219,quick:true}],
 stats:()=>({files:43859,contents:38412,with_meta:21890,not_found:1520,by_format:[["D64",19420],["PRG",11210],["CRT",5260],["T64",1830],["G64",340],["D81",210]]}),
 share_count:()=>ROWS.length, submit_pending_count:()=>0, lists:()=>[{id:1,name:"Klassiker",count:114},{id:2,name:"Neu 2026",count:42}],
 search:(a)=>{const q=(a.options.query||"").toLowerCase();return ROWS.filter(x=>!q||x.title.toLowerCase().includes(q));},
 details:(a)=>DET[a.sha1], game_variants:()=>[], game_note:()=>({note:{note:"",rating:0,done:false},lists:[]}),
 media_data:(a)=>"data/media/"+a.file, check_update:()=>null, community_auto_update:()=>null,
 "plugin:app|version":()=>"0.9.6",
 find_emulators:()=>["/Applications/vice-arm64-sdl2-3.9/x64sc.app"],
};
window.__TAURI__={core:{invoke:async(c,a)=>{__calls.push([c,a]);if(c.startsWith("plugin:dialog"))return null;return R[c]?R[c](a):null}},event:{listen:async()=>()=>{}}};
</script>
"""

# (Name, Bereich, Vorbereitung im Browser) – der Name passt zur Zuordnung in .github/anleitung.py
SHOTS = [
    ("hauptfenster", None, ""),
    ("quellen", 'section[data-key="sources"]', ""),
    ("geraete", 'section[data-key="devices"]', ""),
    ("liste", "#toolbar", ""),
    ("details", "#details", ""),
    ("starten", "#details .launch", ""),
    ("eigene", "#own-dialog", f"openOwnEditor(['{SEL}'], DET['{SEL}'])"),
]


def build(tmp: pathlib.Path, data: pathlib.Path) -> None:
    html = (ROOT / "ui" / "index.html").read_text(encoding="utf-8")
    html = html.replace('href="style.css"', 'href="/ui/style.css"').replace('src="icon.png"', 'src="/ui/icon.png"')
    for js in ("i18n.js", "versionen.js", "app.js"):
        html = html.replace(f'<script src="{js}"></script>', f'<script src="/ui/{js}"></script>')
    html = html.replace('<script src="/ui/i18n.js">', MOCK + '<script src="/ui/i18n.js">', 1)
    (tmp / "seite.html").write_text(html, encoding="utf-8")
    shutil.copytree(ROOT / "ui", tmp / "ui")
    shutil.copytree(data, tmp / "data")


def main() -> None:
    data = pathlib.Path(sys.argv[1]).resolve()
    OUT.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory() as t:
        tmp = pathlib.Path(t)
        build(tmp, data)
        handler = lambda *a, **k: http.server.SimpleHTTPRequestHandler(*a, directory=str(tmp), **k)  # noqa: E731
        server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), handler)
        threading.Thread(target=server.serve_forever, daemon=True).start()
        port = server.server_address[1]
        with sync_playwright() as p:
            browser = p.chromium.launch(channel="chrome")  # installiertes Google Chrome
            for lang in ("de", "en"):
                page = browser.new_page(viewport={"width": 1280, "height": 800}, device_scale_factor=2,
                                        color_scheme="light", locale="de-DE" if lang == "de" else "en-GB")
                page.goto(f"http://127.0.0.1:{port}/seite.html?lang={lang}")
                page.wait_for_function("document.querySelectorAll('#list tbody tr').length > 5", timeout=15000)
                page.evaluate("document.querySelector('#search').value='Archon'; "
                              "document.querySelector('#search').dispatchEvent(new Event('input'))")
                page.wait_for_timeout(700)
                page.evaluate(f"showDetails('{SEL}')")
                page.wait_for_selector("#details .launch", timeout=10000)
                page.wait_for_timeout(1200)  # Bilder und Gerätestatus
                for name, sel, prep in SHOTS:
                    if prep:
                        page.evaluate(prep)
                        page.wait_for_timeout(500)
                    ziel = OUT / f"{name}-{lang}.jpg"
                    if sel:
                        page.locator(sel).first.screenshot(path=str(ziel), type="jpeg", quality=82)
                    else:
                        page.screenshot(path=str(ziel), type="jpeg", quality=82)
                    if prep:
                        page.keyboard.press("Escape")
                    print(ziel.relative_to(ROOT))
                page.close()
            browser.close()
        server.shutdown()


if __name__ == "__main__":
    main()
