# DeckDriod Hotkey Redesign

## Current Problems
- **Inconsistent mnemonics**: `s` = screenshot, `i` = settings (why not `s`?)
- **Conflicts**: `c` = clear logs (should be capture/screenshot)
- **Hard to remember**: `u` = deep link, `b` = layout bounds
- **Shift overload**: `B`/`M`/`E`/`L` require Shift (slow)
- **Missing**: No device switcher, no variant selector

---

## New Scheme — Mnemonic Groups

### 📱 **Build & Launch** (Left hand, frequent)
| Key | Action | Mnemonic |
|-----|--------|----------|
| `r` | **R**ebuild & Launch | **R**un |
| `f` | **F**orce Rebuild (clean) | **F**resh |
| `l` | **L**aunch Only | **L**aunch |
| `k` | **K**ill App | **K**ill |

### 📸 **Capture** (C-group)
| Key | Action | Mnemonic |
|-----|--------|----------|
| `c` | **C**apture Screenshot | **C**apture |
| `v` | **V**ideo Record (toggle) | **V**ideo |

### 🔧 **Settings & Config** (S-group)
| Key | Action | Mnemonic |
|-----|--------|----------|
| `s` | **S**ettings | **S**ettings |
| `d` | **D**evice Switcher | **D**evice |
| `p` | **P**roject Switcher | **P**roject |
| `t` | Variant Selec**t**or | Targe**t** |

### 🔍 **Search & Filter** (Slash-group)
| Key | Action | Mnemonic |
|-----|--------|----------|
| `/` | Search Logs | Vim standard |
| `n` | **N**ext Match | Vim standard |
| `N` | Previous Match | Vim standard |
| `*` | Search Word Under Cursor | Vim standard |

### 📊 **Views** (Numbers)
| Key | Action | Mnemonic |
|-----|--------|----------|
| `1` | Dashboard | Tab 1 |
| `2` | App Logs | Tab 2 |
| `3` | Build Logs | Tab 3 |
| `4` | Errors | Tab 4 |
| `Tab` | Next Tab | Standard |
| `Shift+Tab` | Previous Tab | Standard |

### 🎮 **Actions** (Right hand)
| Key | Action | Mnemonic |
|-----|--------|----------|
| `x` | Clear App Data | **X** = delete |
| `u` | Open Deep **U**RL | **U**RL |
| `b` | Toggle Layout **B**ounds | **B**ounds |
| `m` | Toggle **M**CP Server | **M**CP |
| `e` | **E**xport Logs | **E**xport |
| `w` | Toggle Auto-**W**atch | **W**atch |
| `o` | Toggle Auto-**O**pen | **O**pen |

### 📋 **Copy** (Y-group, Vim-style)
| Key | Action | Mnemonic |
|-----|--------|----------|
| `y` | **Y**ank Line/Selection | Vim **y**ank |
| `Y` | **Y**ank All Logs | Vim **Y** |
| `C` | **C**opy Crash Trace | **C**rash |

### 🧭 **Navigation**
| Key | Action | Mnemonic |
|-----|--------|----------|
| `j` / `↓` | Scroll Down | Vim standard |
| `k` / `↑` | Scroll Up | Vim standard |
| `g` | Go to Top | Vim standard |
| `G` | Go to Bottom / Follow | Vim standard |
| `Ctrl+d` | Page Down | Vim standard |
| `Ctrl+u` | Page Up | Vim standard |

### ℹ️ **Help & Info**
| Key | Action | Mnemonic |
|-----|--------|----------|
| `?` | Help | Standard |
| `q` | Quit | Standard |
| `Esc` | Cancel / Back | Standard |

---

## Migration Map (Old → New)

| Old | New | Action | Reason |
|-----|-----|--------|--------|
| `a` | `r` | Build & Launch | **R**un is clearer |
| `s` | `c` | Screenshot | **C**apture is mnemonic |
| `i` | `s` | Settings | **S**ettings is obvious |
| `c` | `x` + `Ctrl+l` | Clear Logs | `x` = clear data, `Ctrl+l` = clear logs |
| `L` | `l` | Launch Only | Remove Shift |
| `B` | `Shift+d` | Broadcast | Move to device menu |
| `M` | `m` | MCP Toggle | Keep lowercase |
| `E` | `d` then `e` | Emulator | Via device switcher |
| `h` | `?` | Help | Standard |
| `d` | `Shift+m` | Dev Menu | Less common |

---

## New Features (Not Yet Implemented)

| Key | Action | Mnemonic |
|-----|--------|----------|
| `d` | **D**evice Switcher | **D**evice |
| `p` | **P**roject Switcher | **P**roject |
| `t` | Variant Selec**t**or | **T**arget |
| `k` | **K**ill App | **K**ill |
| `n` | **N**ext Search Match | Vim |
| `N` | Previous Match | Vim |
| `*` | Search Word | Vim |
| `Ctrl+l` | Clear Logs | Terminal standard |
| `Ctrl+r` | Regex Search Toggle | Reverse-i-search |

---

## Grouped Cheat Sheet (For UI)

```
┌─ BUILD ─────────────────┬─ CAPTURE ────────────┬─ SETTINGS ───────────┐
│ r  Run (build+launch)   │ c  Capture (screen)  │ s  Settings          │
│ f  Force rebuild        │ v  Video record      │ d  Device switcher   │
│ l  Launch only          │                      │ p  Project switcher  │
│ k  Kill app             │                      │ t  Variant selector  │
├─ VIEWS ─────────────────┼─ ACTIONS ────────────┼─ COPY ───────────────┤
│ 1  Dashboard            │ x  Clear app data    │ y  Yank line         │
│ 2  App logs             │ u  Deep URL          │ Y  Yank all          │
│ 3  Build logs           │ b  Layout bounds     │ C  Copy crash        │
│ 4  Errors               │ m  MCP server        │                      │
│ Tab  Next tab           │ e  Export logs       │                      │
├─ SEARCH ────────────────┼─ NAV ────────────────┼─ HELP ───────────────┤
│ /  Search               │ j/k  Scroll          │ ?  Help              │
│ n  Next match           │ g/G  Top/Bottom      │ q  Quit              │
│ *  Search word          │ Ctrl+d/u  Page       │ Esc  Cancel          │
└─────────────────────────┴──────────────────────┴──────────────────────┘
```

---

## Implementation Plan

### Phase 1: Non-Breaking Changes (v1.2.0)
- Add `?` as alias for help (keep `h`)
- Add `Ctrl+l` for clear logs (keep `c`)
- Add `n`/`N` for search navigation
- Add `k` for kill app

### Phase 2: Breaking Changes (v2.0.0)
- Swap `s` ↔ `c` (screenshot ↔ settings)
- Change `a` → `r` for run
- Remove `L`/`B`/`E` Shift keys
- Add device/project/variant switchers

### Phase 3: Vim Mode (v2.1.0)
- Add `*` for search word under cursor
- Add `Ctrl+d`/`Ctrl+u` for page scroll
- Add `:` command mode for power users

---

## Rationale

### Why Mnemonic Groups?
- **Muscle memory**: Related actions near each other
- **Discoverability**: `c` = capture, `v` = video (both media)
- **Consistency**: All settings under `s`/`d`/`p`/`t`

### Why Remove Shift Keys?
- **Speed**: Lowercase is faster (no modifier)
- **Ergonomics**: Shift + letter is awkward
- **Frequency**: Common actions should be easy

### Why Vim-style Navigation?
- **Familiarity**: Developers know Vim
- **Efficiency**: `hjkl` keeps hands on home row
- **Power**: `gg`, `G`, `Ctrl+d` are muscle memory

---

## User Migration

### Show Deprecation Warnings (v1.2.0)
```
[warn] 'a' is deprecated, use 'r' for Run
[warn] 's' will change to Settings in v2.0, use 'c' for Capture
```

### Add Compatibility Mode (v1.2.0 - v2.0.0)
```ini
# .deckdriodconfig
LEGACY_HOTKEYS=true  # Keep old bindings
```

### Update Help Screen (v1.2.0)
Show both old and new keys:
```
r (was: a)  Build & Launch
c (was: s)  Capture Screenshot
s (was: i)  Settings
```

---

## Testing Checklist

- [ ] All old keys still work in v1.2.0
- [ ] New keys work alongside old keys
- [ ] Help screen shows both old/new
- [ ] Deprecation warnings appear
- [ ] Config flag disables warnings
- [ ] v2.0.0 removes old keys
- [ ] Migration guide in CHANGELOG
