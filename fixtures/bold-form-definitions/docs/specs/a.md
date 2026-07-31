# Bold-form definitions

**[direct-colon-form]**: An input MUST be recognized as a definition
when the closing `**` is directly followed by a colon.

```claim
kind: constraint
evaluator: test
```

**[parenthetical-form]** (P8): An input MUST also be recognized when a
parenthetical sits between the closing `**` and the colon.

```claim
kind: constraint
evaluator: test
```

**[parenthetical-non-ascii-form]** (P9′): The parenthetical MAY hold
non-ASCII content, such as a prime mark.

```claim
kind: constraint
evaluator: test
```

**[italic-amended-form]** _(amended 2026-07-14)_: An input MUST also
be recognized when an italicized revision note — not a plain
parenthetical — sits between the closing `**` and the colon.

```claim
kind: constraint
evaluator: test
```

**[italic-retired-form]** _(retired 2026-07-08 — superseded by the
amended form above)_: The revision note MAY span more than one line
and hold an em dash, matching the real-corpus shape this form exists
for.

```claim
kind: constraint
evaluator: test
```
