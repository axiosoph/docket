# Spec

### [target-claim]

A claim other sections depend on.

```claim
kind: requirement
evaluator: test
```

## Empty sibling

<a id="tied-target"></a>

## Tied target

An equal one-blank-line gap sits on both sides of the anchor above —
two candidate headings tied on raw distance. Binding to "Empty sibling"
(the wrong, first-match answer) would collapse this section's own scope
to nothing, since an empty heading's own "next heading" is this one;
binding here, correctly, keeps the depends entry below satisfied. See
[the target claim](#target-claim).

```claim
kind: requirement
evaluator: test
depends: [target-claim]
```

## Far sibling



<a id="near-target"></a>
## Near target

The anchor above sits three blank lines from "Far sibling" but zero
blank lines from this heading — an asymmetric gap, proving actual
distance decides the nearest heading rather than document order alone.
See [the target claim](#target-claim).

```claim
kind: requirement
evaluator: test
depends: [target-claim]
```
