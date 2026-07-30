# System overview

### [system-boundary]

The system boundary sits between the atom store and the build graph, per
[lock-groundness](lock-groundness). Deleting `lock-groundness` would not
make this architectural fact false — the boundary would still sit where
it sits — only its stated motivation would need restating, so this is a
`because`, not a `depends`.

```claim
kind: requirement
evaluator: none
because: [lock-groundness]
```
