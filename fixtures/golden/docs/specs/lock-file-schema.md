# Lock file schema

### [lock-groundness]

Every lock value MUST be ground: names bound to content identities and
exact version strings. See [the composition model](composition-model#6)
and [the execution model](execution-model#2.4).

```claim
kind: constraint
evaluator: property-test
cites: [composition-model#6, execution-model#2.4]
```
