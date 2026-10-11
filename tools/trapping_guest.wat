;; A guest that stops by its own hand, not by hitting a sandbox limit.
;;
;; `unreachable` is a guest trap: no fuel budget, no deadline, and no page limit
;; is involved. It is the negative control for `infinite_loop`, which stops for
;; the opposite reason. Together they let a test tell "the sandbox intervened"
;; from "the module is broken" — a distinction the error text used to erase,
;; because every trap was wrapped in an error beginning `ResourceExhausted: `.
;;
;; Note there is no `memory` export. That is deliberate: the point is a pure
;; control-flow trap, so a page-budget test can never explain it away.
(module
  (func (export "execute") (result i32)
    unreachable))