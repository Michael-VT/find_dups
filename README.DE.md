# find_dups: Mehrsprachiger Duplikat-Finder

Ein umfassender Duplikat-Finder, implementiert in Go, Python, Rust, JavaScript und C++ mit identischen Algorithmen für Leistungsvergleich und Produktionseinsatz.

## Überblick

`find_dups` scannt ein oder mehrere Verzeichnisse rekursiv, identifiziert doppelte Dateien mittels SHA-256-Hashing und erstellt Berichte sowie Löskripte. Es verwendet parallele Verarbeitung zur effizienten Handhabung großer Dateisammlungen.

### Hauptfunktionen

- **Mehrsprachige Implementierung**: Go-, Python-, Rust-, JavaScript- und C++-Versionen für Leistungsvergleich
- **Paralleles Hashing**: Nutzt alle CPU-Kerne für schnelle Duplikaterkennung
- **Sicherheit**: Erstellt ein Löskript anstatt Dateien direkt zu löschen
- **Detaillierte Berichte**: CSV-Exporte mit Dateimetadaten und Zeitstempeln
- **Laufwerksübergreifend**: Kann mehrere Verzeichnisse über verschiedene Mount-Punkte scannen

## Algorithmus

Alle fünf Implementierungen folgen demselben Algorithmus:

1. **Dateien sammeln** — Rekursiver Durchlauf aller angegebenen Verzeichnisse, Aufzeichnung von Pfad, Größe, Erstellungs- und Änderungszeit
2. **Nach Größe gruppieren** — Nur Dateien, die ihre Größe mit mindestens einer anderen Datei teilen, werden zum Hashing fortgesetzt (Optimierung)
3. **Paralleles SHA-256-Hashing** — Berechnung kryptografischer Hashes parallel:
   - Go: Goroutines mit Channel-basiertem Worker-Pool
   - Python: `multiprocessing.Pool`
   - Rust: `rayon` paralleler Iterator
   - JavaScript: `worker_threads` mit Worker-Pool
   - C++: `std::thread` mit Thread-Pool
4. **Duplikate identifizieren** — Gruppierung von Dateien nach Hash innerhalb von Gruppengrößen; alle Dateien in einer Hash-Gruppe mit >1 Element sind Duplikate
5. **Ausgaben generieren**:
   - `duplicates_<lang>.csv` — Alle Duplikatgruppen mit vollen Metadaten
   - `sort_dup_<lang>.csv` — Alle Dateien nach Größe sortiert (absteigend)
   - `duprm_<lang>.sh` — Bash-Skript, das alle Duplikate außer der ersten (nach ID) in jeder Gruppe löscht

## Ausgabedateien

### duplicates_<lang>.csv
CSV-Datei mit allen nach Inhalt gruppierten Duplikaten. Spalten:
- `FileID`: Fortlaufende Dateikennung
- `Path`: Vollständiger Dateipfad
- `Size`: Dateigröße in Bytes
- `Hash`: SHA-256-Hash (hexadezimal)
- `CreationTime`: Dateierstellungszeitstempel (ISO 8601)
- `ModificationTime`: Dateiänderungszeitstempel (ISO 8601)

### sort_dup_<lang>.csv
CSV-Datei mit allen gescannten Dateien, nach Größe sortiert (absteigend). Gleiche Spalten wie `duplicates_<lang>.csv`.

### duprm_<lang>.sh
Ausführbares Bash-Skript, das Duplikatdateien löscht und die erste Datei (niedrigste FileID) in jeder Duplikatgruppe erhält. **Überprüfen Sie dieses Skript vor der Ausführung**, um sicherzustellen, dass Sie keine wichtigen Dateien löschen.

## Installation & Verwendung

### Go-Implementierung

**Voraussetzungen**: Go 1.16+

**Build**:
```bash
cd find_dups_go
go build -o find_dups_go find_dups_go.go
```

**Ausführen**:
```bash
./find_dups_go /pfad/zum/scan1 /pfad/zum/scan2 ...
```

**Abhängigkeiten**: Nur Standardbibliothek

### Python-Implementierung

**Voraussetzungen**: Python 3.8+

**Ausführen**:
```bash
cd find_dups_pthon
python3 find_dups.py /pfad/zum/scan1 /pfad/zum/scan2 ...
```

**Abhängigkeiten**: Nur Standardbibliothek

### Rust-Implementierung

**Voraussetzungen**: Rust 1.70+, Cargo

**Build**:
```bash
cd find_dups_rust
cargo build --release
```

**Ausführen**:
```bash
./target/release/find_dups /pfad/zum/scan1 /pfad/zum/scan2 ...
```

**Abhängigkeiten** (siehe `Cargo.toml`):
- `walkdir` 2.5 — Verzeichnisdurchlauf
- `sha2` 0.10 — SHA-256-Hashing
- `csv` 1.4 — CSV-Schreiben
- `chrono` 0.4 — Zeitformatierung
- `rayon` 1.12 — Parallelverarbeitung


### JavaScript-Implementierung (Node.js)

**Voraussetzungen**: Node.js 16+ (mit worker_threads Unterstützung)

**Ausführen**:
```bash
cd find_dups_js
node find_dups.js /pfad/zum/scan1 /pfad/zum/scan2 ...
```

**Abhängigkeiten**: Nur Standardbibliothek (`crypto`, `fs`, `worker_threads`)

### C++-Implementierung

**Voraussetzungen**: g++ mit C++17 Unterstützung, OpenSSL (libcrypto)

**Build**:
```bash
cd find_dups_cp
g++ -std=c++17 -O3 -pthread -I/usr/local/opt/openssl/include -L/usr/local/opt/openssl/lib find_dups.cpp -o find_dups_cpp -lcrypto -Wno-deprecated-declarations
```

**Ausführen**:
```bash
./find_dups_cpp /pfad/zum/scan1 /pfad/zum/scan2 ...
```

**Abhängigkeiten**:
- OpenSSL (libcrypto) — SHA-256-Hashing
- Standardbibliothek für Dateisystem und Threads
## Benchmark-Ergebnisse

Getestet mit ca. 149.000 Dateien in zwei Verzeichnissen (lokale SSD + externes USB-Laufwerk):

| Metrik                | Python     | Rust       | Go         |
|-----------------------|------------|------------|------------|
| Dateien gescannt      | 149.044    | 148.819    | 148.819    |
| Dateien gehasht       | 128.964    | 128.738    | 128.738    |
| Hashing-Zeit          | 1:55.751   | 2:07.520   | 1:31.992   |
| Gesamtzeit            | 5:26.243   | 3:55.664   | 3:06.040   |
| Duplikate gefunden    | 0          | 696        | 696        |
| Worker/Threads        | 12         | 12         | 12         |

**Hinweise**:
- Zeiten im Format `Minuten:Sekunden.Millisekunden`
- Die Python-Implementierung fand 0 Duplikate, während Go und Rust 696 fanden, was auf einen möglichen Bug in der Duplikaterkennungslogik der Python-Version hinweist
- Go zeigt die beste Gesamtleistung trotz langsamerer Dateisammlung als Rust

## Bewertung & Empfehlungen

### Stärken

- **Praktischer Wert**: Hoch — löst ein echtes Problem der Duplikatsuche über mehrere Verzeichnisse und Laufwerke hinweg
- **Sicherheit**: Gut — erstellt ein Löskript zur Überprüfung anstatt direkt zu löschen
- **Performance**: Alle drei Implementierungen nutzen parallele Verarbeitung effektiv
- **Transparenz**: CSV-Berichte ermöglichen detaillierte Analyse vor dem Löschen

### Bekannte Probleme

1. **Python-Diskrepanz**: Die Python-Version fand 0 Duplikate, während Go/Rust 696 fanden. Dies erfordert Untersuchung — wahrscheinlich verbunden mit den unterschiedlichen Dateianzahlen (149.044 vs 148.819) oder einem Bug in der Duplikaterkennungslogik.

2. **Plattformbeschränkungen**:
   - Go verwendet macOS-spezifisches `syscall.Stat_t` für Geburtszeit
   - Rust verwendet `std::os::darwin::fs::MetadataExt` für Geburtszeit
   - Beide erfordern bedingte Kompilierung für Linux/Windows-Unterstützung

### Welche Implementierung verwenden?

- **Für Produktionseinsatz auf macOS**: Go — am schnellsten insgesamt, einzelne Binärdatei ohne Abhängigkeiten
- **For plattformübergreifende Entwicklung**: Rust — am einfachsten mit `#[cfg(target_os)]` Attributen anzupassen
- **Für schnelles Skripting/Prototyping**: Python — am einfachsten zu ändern, aber zuerst den Duplikaterkennungs-Bug untersuchen

## Zukunftsvisionen

1. **Python-Duplikaterkennung korrigieren** — Diskrepanz zwischen Implementierungen untersuchen
2. **Fortschrittsanzeige** — Echtzeit-Progress-Anzeige während Hashing-Phase
3. **Plattformübergreifende Geburtszeit** — Bedingte Kompilierung für Linux/Windows
4. **Partielles Hashing-Optimierung** — Hashing der ersten/letzten N KB + Größe vor vollem Dateihash
5. **Konfigurierbarer Output** — Ausgabeverzeichnis und Dateipräfixe angebbar
6. **Interaktiver Modus** — Einfaches TUI zur Überprüfung von Duplikaten vor dem Löschen
7. **Dry-Run-Modus** — Anzeigen, was gelöscht würde, ohne Skript zu generieren
8. **Verschieben statt Löschen** — Option, Duplikate in Staging-Verzeichnis zu verschieben
9. **Mindestgrößen-Filter** — Dateien unter konfigurierbarem Schwellenwert überspringen (z.B. <1KB)
10. **Symlink/Hardlink-Deduplizierung** — Duplikate durch Hardlinks ersetzen um Platz zu sparen ohne zu löschen

## Lizenz

Dieses Projekt wird wie besehen für educative und praktische Nutzung bereitgestellt.

## Mitwirken

Beiträge sind willkommen, besonders für:
- Korrektur des Python-Duplikaterkennungsproblems
- Hinzufügen von Windows/Linux-Kompatibilität
- Implementierung der oben aufgeführten Zukunftsvisionen
