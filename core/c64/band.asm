; VORBEREITET, noch nicht eingebaut: Fern-Start von TAPs scheitert an FW 3.15a (Band stoppt nach der
; Telnet-Aktion, GideonZ/1541ultimate#925) und an JiffyDOS (keine Bandroutinen). Stand 2026-11-03.
;
; Breadbin-Bandlader: lädt die erste Datei vom Band wie LOAD"",1 und startet sie wie ein
; anschließend getipptes RUN. Wird per REST (runners:run_prg) gestartet; danach startet
; Breadbin das Band über das Menü der Ultimate („Start Tape“), das auch die Taste PLAY meldet.
;
; Bauen: 64tass --cbm-prg -a -o band.prg band.asm
;
; Warum nicht „Run Tape“ der Ultimate: Es tippt nur LOAD, und mit JiffyDOS-KERNAL lädt LOAD
; ohne Geräteangabe von Laufwerk 8 statt vom Band.
;
; Die Laderoutine liegt im BASIC-Eingabepuffer ($0200): Kassettenpuffer und $0100 braucht das
; Band selbst, und die geladene Datei liegt meist ab $0801. Sonst wie lader.asm.

SETLFS  = $ffba
SETNAM  = $ffbd
LOAD    = $ffd5
LINKPRG = $a533         ; BASIC-Zeilen neu verketten
READY   = $a474         ; BASIC-Warmstart „READY.“
VARTAB  = $2d           ; Ende des BASIC-Programms
KEYBUF  = $0277
KEYCNT  = $c6
ZIEL    = $0200         ; BASIC-Eingabepuffer, während des Ladens unbenutzt

        * = $0801
        .word ende_basic, 10
        .byte $9e
        .text "2061"
        .byte 0
ende_basic
        .word 0

start   ldx #routine_ende - routine - 1
-       lda routine,x
        sta ZIEL,x
        dex
        bpl -
        jmp ZIEL

routine
        .logical ZIEL
        lda #0
        sta $0801       ; eigenes Programm „löschen“, damit RUN es nicht neu startet
        sta $0802
        lda #1
        ldx #1          ; Gerät 1 = Datasette
        ldy #0          ; wie LOAD"",1: BASIC-Programme nach $0801
        jsr SETLFS
        lda #0          ; ohne Namen: erste Datei
        jsr SETNAM
        lda #0
        ldx #$01
        ldy #$08
        jsr LOAD
        bcs fertig      ; Fehler: einfach zu READY
        stx VARTAB
        sty VARTAB+1
        lda $0802       ; BASIC-Programm ab $0801? (Zeiger auf nächste Zeile ≠ 0)
        beq fertig
        jsr LINKPRG
        ldx #3
-       lda run,x
        sta KEYBUF,x
        dex
        bpl -
        lda #4
        sta KEYCNT
fertig  jmp READY

run     .byte $52, $55, $4e, $0d        ; R U N ⏎
        .endlogical
routine_ende

        .cerror routine_ende - routine > $58, "Laderoutine passt nicht in den Eingabepuffer"
