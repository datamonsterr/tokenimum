# ADR 0037: `tkm top` layout and keys

Date: 2026-10-29. Status: accepted (epic E8.1).

## Decisions

1. **Layout.** A tab bar lists the six panes in SPEC order. Below it is the active pane, then a
   status line with the mode (`live` or `read-only`), the last message time, and `? help`.
2. **Keys.** `Tab` / `Shift-Tab` and `1`..`6` switch panes. `j`/`k` and the arrow keys move the
   row selection. `Enter` opens the explanation of the selected row in an overlay, and `Esc`
   closes it. `r` reloads the database snapshot. `?` toggles help. `q` or `Ctrl-C` quits.
3. **Explanations.** `Enter` renders the same text as `tkm why` for that row, from
   `tkm_core::explain` over database rows. A row with no stored event says so.
4. **Rendering is a pure function** `tui::render(&MonitorState, &ViewState, &mut Frame)`. Tests
   draw it on `ratatui::backend::TestBackend` with fixed fixtures and compare whole buffers, one
   snapshot per pane plus help, read-only mode and the overlay.
5. **Terminal handling.** `ratatui::init` / `ratatui::restore` and a panic hook that restores the
   terminal. The event loop waits on crossterm input and on the feed channel. It never polls the
   database.
