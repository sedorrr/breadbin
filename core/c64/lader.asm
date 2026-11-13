; Breadbin-Lader: lädt die erste Datei von Laufwerk 8 wie LOAD"*",8,1 und startet sie
; wie ein anschließend getipptes RUN. Wird per REST (runners:run_prg) gestartet,
; nachdem das Disk-Abbild in Laufwerk A eingelegt wurde.
;
; Bauen: 64tass --cbm-prg -a -o lader.prg lader.asm
;
; Ablauf: Die Laderoutine wird in den BASIC-Eingabepuffer ($0200–$0258) kopiert, weil die
; geladene Datei meist ab $0801 liegt und den Lader überschreiben würde. Nicht in den
; Kassettenpuffer: Autostart-Dateien laden sich gern nach $032C–$03FF (Vektoren + Puffer) und
; überschrieben dort den Lader mitten im LOAD (z. B. „Abyssal Karma“). Vor dem Laden wird die erste
; BASIC-Zeile gelöscht; liegt danach wieder ein BASIC-Programm ab $0801, wird RUN
; über den Tastaturpuffer gegeben, sonst bleibt der C64 bei READY. Wie nach einem getippten
; LOAD ruft der Lader danach CLALL ($FFE7) auf – Autostart-Dateien, die den CLALL-Vektor
; ($032C) verbiegen, starten so wie gewohnt selbst.

SETLFS  = $ffba
SETNAM  = $ffbd
LOAD    = $ffd5
LINKPRG = $a533         ; BASIC-Zeilen neu verketten
READY   = $a474         ; BASIC-Warmstart „READY.“
CLALL   = $ffe7         ; alle Dateien schließen (BASIC ruft es nach LOAD über $A52A auf)
VARTAB  = $2d           ; Ende des BASIC-Programms
KEYBUF  = $0277
KEYCNT  = $c6
ZIEL    = $0200         ; BASIC-Eingabepuffer (89 Byte), während des Ladens unbenutzt

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
        ldx #8
        ldy #1
        jsr SETLFS
        lda #1
        ldx #<name
        ldy #>name
        jsr SETNAM
        lda #0
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
fertig  jsr CLALL       ; wie BASIC nach LOAD – startet Autostart über den CLALL-Vektor
        jmp READY

name    .text "*"
run     .byte $52, $55, $4e, $0d        ; R U N ⏎
        .endlogical
routine_ende

        .cerror routine_ende - routine > $58, "Laderoutine passt nicht in den BASIC-Eingabepuffer"
