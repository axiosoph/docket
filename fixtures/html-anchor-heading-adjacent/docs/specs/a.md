# Spec

### [target-claim]

A claim other sections depend on.

```claim
kind: requirement
evaluator: test
```

<a id="lock-sufficiency"></a>
#### Lock sufficiency

The lock MUST pin.

##### A deeper subheading

More detail, still part of the section above — a heading-adjacent
anchor's scope survives a deeper subheading exactly like an ordinary
bracket-kebab heading's does. See [the target claim](#target-claim),
cited from inside the subheading, proving the scope reached this far.

```claim
kind: requirement
evaluator: test
depends: [target-claim]
```

#### Second claim

<a id="second-claim"></a>

The anchor sits AFTER its heading here — the other authoring order,
resolving identically. See [the target claim](#target-claim).

```claim
kind: requirement
evaluator: test
depends: [target-claim]
```
