# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

claudesh is an AI-powered Unix shell written in Rust. It executes standard commands via bash and sends unrecognized (natural language) input to the Claude CLI for command generation. The entire implementation lives in a single file: `src/main.rs` (~1,460 lines).

## Build & Run Commands

- **Build:** `cargo build --release` (or `make build`, which first checks that `claude` CLI is in PATH)
- **Install:** `make install` (builds + copies binary to `/usr/local/bin/claudesh`)
- **Clean:** `cargo clean`
- **Check only:** `cargo check`
- **Run directly:** `cargo run`

There are no automated tests. The project has no CI/CD configuration.

## Prerequisites

The `claude` CLI (Anthropic's Claude Code) must be in PATH. The Makefile's `check-claude` target enforces this before building. At runtime, claudesh invokes `claude --print --no-input --system <prompt>` and passes user input via stdin.

## Architecture

### Single-file design (`src/main.rs`)

The entire shell is one file with clearly separated sections:

| Section | Approx. Lines | Responsibility |
|---|---|---|
| Constants & config structs | 1–64 | Default prompts (embedded via `include_str!`), ANSI colors, `Config`/`RunResult` structs |
| `main()` + execution modes | 65–315 | Entry point: `-c` flag, script file, piped stdin, or interactive REPL |
| Interactive REPL | 317–474 | Rustyline-based read-eval loop with history, confirmation flow, error handling |
| Config loading | 478–545 | Reads `~/.claudesh/` files, falls back to embedded defaults |
| Input classification | 546–715 | Classifies input into `InputKind` enum (12 variants) — determines if input is a builtin, shell command, or natural language |
| Bash execution | 716–780 | Spawns `bash -c`, tee's stderr (real-time display + capture for error analysis, capped at 1MB) |
| Shell builtins | 784–976 | `cd`, `export`, `unset`, `source`, `history` — handled in-process, not delegated to bash |
| Claude integration | 1015–1312 | Calls `claude` CLI for generate/explain/ask/fix/script operations |
| UI/output helpers | 1314–1458 | Prompt rendering, colored output, help text |

### Input classification flow (`classify_input`)

The heuristic in `classify_input()` (line ~565) determines whether user input is a command or natural language:
1. Comments (`#`), exit/quit, help, history → special handling
2. Prefix-based: `!` (force bash), `??` (ask Claude), `?` (explain command)
3. Shell builtins: cd, export, unset, source
4. `is_shell_command()` check: shell syntax chars (`/`, `~`, `$`, `(`, etc.), variable assignments (`FOO=bar`), command prefixes (`sudo`, `env`, etc.), commands found in PATH
5. Fallback → natural language, sent to Claude

### Claude CLI integration

All AI features invoke the external `claude` binary (not an HTTP API directly). The pattern is:
```
Command::new("claude").args(["--print", "--no-input", "--system", &system_prompt])
```
User input is piped to stdin; raw text output is captured from stdout. The personality prompt is appended to system prompts for explain/ask/fix but **not** for command generation (to keep generated commands clean).

### Embedded defaults (`defaults/` directory)

Prompt templates and personality are embedded at compile time via `include_str!()`. On first run, these are written to `~/.claudesh/` where users can customize them. The `defaults/prompts/` directory contains: `generate.txt`, `explain.txt`, `ask.txt`, `fix.txt`, `script.txt`.

## Dependencies

Only three crates: **rustyline** (line editing + history), **which** (PATH resolution), **dirs** (home directory discovery).
