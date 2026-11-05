# herdr-tabshell

A herdr plugin that gives each herdr tab its own persistent shell, shown in a floating window on a key press.

## Language

**Tab shell**:
The tmux window bound to one herdr tab. It keeps running while hidden and after its tab closes.
_Avoid_: scratch, popup, floax, floating shell

**Shell float**:
The herdr float that belongs to one workspace and displays the tab shell of that workspace's current tab. Each workspace has at most one. It is absent, hidden, or shown, and appears only on its own workspace.
_Avoid_: popup, scratch window

**Binding**:
The link from a herdr tab to its tab shell, held by tab ID and never by name.
_Avoid_: mapping, association
