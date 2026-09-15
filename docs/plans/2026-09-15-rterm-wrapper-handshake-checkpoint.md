# Windows outer-kill fixture handshake

PR CI 34929435133 at `1b5de896` failed only the outer-kill descendant test:
its eight-second wait did not observe the PID sentinel. Push CI 34929431524,
both R-Term candidate/rollback jobs, and Functional passed. The hosted log
does not distinguish wrapper compilation delay from descendant startup delay.

The unchanged test passed locally. Adding a ten-second delay before the
descendant's PowerShell PID write reproduced the same failure in 8.03 seconds.
That exposed an unnecessary dependency: the job-owning launcher already has
the process identity, but the test waited for the descendant to publish it.

`Invoke-BoundedProcess` now has an optional test-only started-process observer.
It runs after job assignment/resume, inside the existing cleanup scope. The
outer-kill fixture verifies that the owned process has not exited and atomically
publishes its PID from that handle. The descendant only sleeps; it never
participates in readiness. The real outer kill and descendant-exit assertion
remain. Startup failures now include wrapper stderr rather than hiding it.

The eight-second handshake and two-second outer-kill deadlines are unchanged.
Exit probes share the existing ten-second budget and use `ChildGuard` instead
of an unbounded PowerShell wait. A probe timeout is an error, not evidence of
absence; an already-zero budget is rejected before spawning. `ChildGuard`'s
existing bounded cleanup grace (up to 500 ms) is additional to its probe budget.

This fixes the demonstrated handshake defect, not every possible cause of a
slow hosted wrapper. New committed-SHA CI is still needed. No product runtime,
performance threshold, frozen ref, or Task 12 draft was changed, and no merge
or physical split is performed by this checkpoint.
