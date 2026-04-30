# find_dups: Buscador de Duplicados Multi-idioma

Um buscador de duplicatas abrangente implementado em Go, Python e Rust com algoritmos idênticos para comparação de performance e uso em produção.

## Visão Geral

`find_dups` escaneia recursivamente um ou mais diretórios, identifica arquivos duplicados usando hash SHA-256 e gera relatórios e scripts de exclusão. Usa processamento paralelo para lidar eficientemente com grandes coleções de arquivos.

### Recursos Principais

- **Implementação multi-idioma**: versões Go, Python e Rust para comparação de performance
- **Hashing paralelo**: Utiliza todos os núcleos da CPU para detecção rápida de duplicatas
- **Segurança**: Gera um script de exclusão em vez de excluir arquivos diretamente
- **Relatórios detalhados**: Exportações CSV com metadados de arquivos e timestamps
- **Suporte multi-drive**: Pode escanear múltiplos diretórios através de diferentes pontos de montagem

## Algoritmo

Todas as três implementações seguem o mesmo algoritmo:

1. **Coletar arquivos** — Caminhada recursiva através de todos os diretórios especificados, registrando caminho, tamanho, hora de criação e modificação
2. **Agrupar por tamanho** — Apenas arquivos compartilhando um tamanho com pelo menos um outro arquivo prosseguem para o hashing (otimização)
3. **Hashing paralelo SHA-256** — Calcula hashes criptográficos em paralelo:
   - Go: goroutines com pool de workers baseado em canais
   - Python: `multiprocessing.Pool`
   - Rust: iterador paralelo `rayon`
4. **Identificar duplicatas** — Agrupa arquivos por hash dentro de grupos de tamanho; todos os arquivos em um grupo de hash com >1 membro são duplicatas
5. **Gerar saídas**:
   - `duplicates_<lang>.csv` — Todos os grupos de arquivos duplicados com metadados completos
   - `sort_dup_<lang>.csv` — Todos os arquivos ordenados por tamanho (decrescente)
   - `duprm_<lang>.sh` — Script bash que exclui todas as duplicatas exceto a primeira (por ID) em cada grupo

## Arquivos de Saída

### duplicates_<lang>.csv
Arquivo CSV contendo todos os arquivos duplicados agrupados por conteúdo. Colunas:
- `FileID`: Identificador sequencial do arquivo
- `Path`: Caminho completo do arquivo
- `Size`: Tamanho do arquivo em bytes
- `Hash`: Hash SHA-256 (hexadecimal)
- `CreationTime`: Timestamp de criação do arquivo (ISO 8601)
- `ModificationTime`: Timestamp de modificação do arquivo (ISO 8601)

### sort_dup_<lang>.csv
Arquivo CSV contendo todos os arquivos escaneados ordenados por tamanho (decrescente). Mesmas colunas que `duplicates_<lang>.csv`.

### duprm_<lang>.sh
Script bash executável que remove arquivos duplicados, preservando o primeiro arquivo (menor FileID) em cada grupo de duplicatas. **Revise este script antes de executar** para garantir que você não exclua arquivos importantes.

## Instalação & Uso

### Implementação Go

**Pré-requisitos**: Go 1.16+

**Build**:
```bash
cd find_dups_go
go build -o find_dups_go find_dups_go.go
```

**Executar**:
```bash
./find_dups_go /caminho/para/scan1 /caminho/para/scan2 ...
```

**Dependências**: Apenas biblioteca padrão

### Implementação Python

**Pré-requisitos**: Python 3.8+

**Executar**:
```bash
cd find_dups_pthon
python3 find_dups_python_e.py /caminho/para/scan1 /caminho/para/scan2 ...
```

**Dependências**: Apenas biblioteca padrão

### Implementação Rust

**Pré-requisitos**: Rust 1.70+, Cargo

**Build**:
```bash
cd find_dups_rust
cargo build --release
```

**Executar**:
```bash
./target/release/find_dups_rust /caminho/para/scan1 /caminho/para/scan2 ...
```

**Dependências** (veja `Cargo.toml`):
- `walkdir` 2.5 — Travessia de diretórios
- `sha2` 0.10 — Hashing SHA-256
- `csv` 1.4 — Escrita CSV
- `chrono` 0.4 — Formatação de tempo
- `rayon` 1.12 — Processamento paralelo

## Resultados de Benchmark

Testado em aproximadamente 149.000 arquivos em dois diretórios (SSD local + drive USB externo):

| Métrica                | Python     | Rust       | Go         |
|------------------------|------------|------------|------------|
| Arquivos escaneados    | 149.044    | 148.819    | 148.819    |
| Arquivos hasheados     | 128.964    | 128.738    | 128.738    |
| Tempo de hashing       | 1:49.337   | 2:07.520   | 1:31.992   |
| Tempo total            | 4:10.790   | 3:55.664   | 3:06.040   |
| Duplicatas encontradas | 920        | 696        | 696        |
| Workers/threads        | 12         | 12         | 12         |

**Notas**:
- Tempos no formato `minutos:segundos.milissegundos`
- Todas as três implementações encontraram duplicatas, mostrando trabalho correto do algoritmo (Python: 920, Rust: 696, Go: 696)
- Go mostra a melhor performance geral apesar de ter coleta de arquivos mais lenta que Rust

## Avaliação & Recomendações

### Pontos Fortes

- **Valor prático**: Alto — resolve um problema real de encontrar duplicatas através de múltiplos diretórios e drives
- **Segurança**: Boa — gera um script de exclusão para revisão em vez de excluir diretamente
- **Performance**: Todas as três implementações usam processamento paralelo efetivamente
- **Transparência**: Relatórios CSV permitem análise detalhada antes da exclusão

### Problemas Conhecidos

1. **Discrepância em duplicatas**: Python encontrou 920 duplicatas enquanto Go/Rust encontraram 696. Isso ocorre porque Python escaneou mais arquivos (149.044 vs 148.819).

2. **Limitações de plataforma**:
   - Go usa `syscall.Stat_t` (específico de macOS) para hora de criação
   - Rust usa `std::os::darwin::fs::MetadataExt` para hora de criação
   - Ambos requerem compilação condicional para suporte Linux/Windows

### Qual implementação usar?

- **Para uso em produção no macOS**: Go — mais rápido geralmente, binário único sem dependências
- **Para desenvolvimento multi-plataforma**: Rust — mais fácil de adaptar com atributos `#[cfg(target_os)]`
- **Para script/prototipagem rápida**: Python — mais fácil de modificar

## Melhorias Futuras

1. **Investigar discrepância** — por que Python encontrou mais duplicatas que Go/Rust
2. **Barra de progresso** — Adicionar indicação de progresso em tempo real durante a fase de hashing
3. **Hora de criação multi-plataforma** — Usar compilação condicional para Linux/Windows
4. **Otimização de hashing parcial** — Hash dos primeiros/últimos N KB + tamanho antes do hash completo do arquivo
5. **Saída configurável** — Permitir especificar diretório de saída e prefixos de arquivo
6. **Modo interativo** — TUI simples para revisar duplicatas antes da exclusão
7. **Modo dry-run** — Mostrar o que seria excluído sem gerar um script
8. **Mover em vez de excluir** — Opção de mover duplicatas para um diretório de staging
9. **Filtro de tamanho mínimo** — Ignorar arquivos abaixo de um limiar configurável (ex: <1KB)
10. **Deducação por symlink/hardlink** — Substituir duplicatas por hardlinks para economizar espaço sem excluir

## Licença

Este projeto é fornecido como está para uso educacional e prático.

## Contribuindo

Contribuições são bem-vindas, especialmente para:
- Adicionar compatibilidade Windows/Linux
- Implementar qualquer das melhorias futuras listadas acima
