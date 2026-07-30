# Lock file schema

### [lock-groundness]

Every lock value MUST be ground: names bound to content identities and
exact version strings. See [the composition model](../models/composition-model.md#6)
and [the execution model](../models/execution-model.md#2.4).

```claim
kind: constraint
evaluator: property-test
cites: [docs/models/composition-model#6, docs/models/execution-model#2.4]
```
