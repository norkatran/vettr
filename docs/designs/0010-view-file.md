# 0010 View file modal

Status: Fulfilled (not yet compiled or run: no Rust toolchain was available when written)

## Problem
Reviewing a diff sometimes needs the whole file for context, but the app only shows changes.

## Decision
The 3-dot menu in each file header gets a "View file" item (also available in read-only views). It opens a modal `egui::Window` showing the file's current contents, read-only, scrollable both ways, with line numbers and syntax highlighting. Nothing external is launched.

- `Backend::read_project_file` reads the file (must stay inside the project, max 2 MB, UTF-8 text only; otherwise the modal shows why it cannot be shown).
- `ChangesModel::view_file` splits it into lines and highlights it with `highlight_hunk` by treating every line as context. Rows are virtualised with `ScrollArea::show_rows`.
- Closed with the window's close button or Escape. Disabled for deleted files.

Out of scope: editing, viewing old revisions, jumping to a diff line.

## To do
- [x] Backend file read
- [x] Viewer state and modal
- [x] Menu item
- [ ] Compile and try it (cargo was unavailable)
