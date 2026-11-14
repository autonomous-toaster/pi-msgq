# Tasks

## 1. The compact rendering

- [x] 1.1 Add `card::summary_compact` rendering `project/branch  basename`. Verify a card with every field renders as decided. Verify a card with no project, a card with no path and an empty card each render as decided.
- [x] 1.2 Verify the compact form never contains a tab, and that `--wide` still prints the full path beside it.

## 2. The card form on both listings

- [x] 2.1 Add `--card` to `topics` and `sessions`, reading the roster once by address. Verify each prints `address<TAB>description`. Verify `-` stands where no participant advertises the address.
- [x] 2.2 Verify `--card` conflicts with `--names` and with `--json` on both listings (usage error, exit 2, no listing printed).
- [x] 2.3 Verify the card form costs no extra read: `topics --card` on the shared engine performs the same one catalogue call plus one presence read that `sessions` already performs.

## 3. The picking recipe

- [x] 3.1 Document both recipes in README — `topics --card | fzf --delimiter='\t' --with-nth=2.. | cut -f1` and the `sessions` equivalent — and state that the first column is what is selected while the second is what is shown.
- [x] 3.2 Verify the documented recipes run as written against a live bus on both engines.

## 4. A row that fits

- [x] 4.1 Clip each part of the description to its budget with the middle replaced by `…`. Verify a long project, a long branch and a long directory each stay inside the budget. Verify a short name keeps every character.
- [x] 4.2 Verify in a terminal narrower than the row that the address stays visible. Verify that the description is what truncates. Record the recipe in README without `--with-nth`.
