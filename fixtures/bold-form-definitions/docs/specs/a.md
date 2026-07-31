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
