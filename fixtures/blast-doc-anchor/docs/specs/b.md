### [depends-transitively]

Builds on [depends-on-rule](depends-on-rule), which cites the rule
directly — so this claim's blast radius must reach it too, transitively,
through a document-anchor citer that is not a graph dead end.

```claim
kind: constraint
evaluator: test
depends: [depends-on-rule]
```
