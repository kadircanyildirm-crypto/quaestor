Output "/out/quaestor-demo.mp4"

Require quaestor-cli

Set Shell "bash"
Set FontFamily "JetBrains Mono"
Set FontSize 22
Set Width 1920
Set Height 1080
Set Padding 36
Set Theme "Catppuccin Mocha"
Set WindowBar Colorful
Set TypingSpeed 30ms
Set Framerate 24
Set WaitTimeout 180s
Set WaitPattern /\$ ?$/
Set CursorBlink false

Hide
Type "source /root/demo.bashrc && clear"
Enter
Wait
Show

# ── intro ────────────────────────────────────────────────────────────────
Type "# quaestor: a zero-knowledge proof that your grade was computed from the answer key"
Enter
Type "# the school committed to BEFORE the exam, without ever revealing that key."
Enter
Sleep 1.5s
Type "# The grading core and its 84 tests (incl. property-based):"
Enter
Type `cargo test -q --manifest-path ~/quaestor/Cargo.toml 2>&1 | grep "test result"`
Enter
Wait
Sleep 3.5s
Ctrl+L

# ── inputs ───────────────────────────────────────────────────────────────
Type "# The school's private answer key (never published) and one student's answer sheet:"
Enter
Type "cat key.json"
Enter
Wait
Sleep 2.5s
Type "cat sheet.json"
Enter
Wait
Sleep 3s
Ctrl+L

# ── execute ──────────────────────────────────────────────────────────────
Type "# Mock prover: the guest really runs in the zkVM and every check below is real,"
Enter
Type "# but SP1's cryptography is skipped. A real proof needs a 24+ GB machine."
Enter
Type "export SP1_PROVER=mock"
Enter
Sleep 1.5s
Type "# 1) Grade the sheet inside the SP1 zkVM (RISC-V):"
Enter
Type "quaestor-cli execute --key key.json --salt $SALT --sheet sheet.json"
Enter
Wait
Sleep 4s
Type "# The school publishes this key commitment before the exam. Students copy it:"
Enter
Type@4ms "C=d4f3126b73e9d22c7131c60e28b73a787b340a935d55af42fb04775c9bd51d0c"
Enter
Sleep 2.5s
Ctrl+L

# ── prove + verify ───────────────────────────────────────────────────────
Type "# 2) After the exam: prove the grading. The key and the salt stay private."
Enter
Type "quaestor-cli prove --key key.json --salt $SALT --sheet sheet.json --out proof.bin"
Enter
Sleep 1.5s
Hide
Wait
Show
Sleep 3s
Type "# 3) The student verifies with public data only: the proof, the commitment, their own answers:"
Enter
Type "quaestor-cli verify --proof proof.bin --commitment $C --sheet sheet.json"
Enter
Wait
Sleep 4.5s
Ctrl+L

# ── forgery 1 ────────────────────────────────────────────────────────────
Type "# Forgery 1: present the same proof as if it were about another student's answers"
Enter
Type "quaestor-cli verify --proof proof.bin --commitment $C --sheet other-sheet.json"
Enter
Wait
Sleep 4s
Hide
Type "true"
Enter
Wait
Ctrl+L
Sleep 300ms
Show

# ── forgery 2 ────────────────────────────────────────────────────────────
Type "# Forgery 2: after the exam, the school quietly changes the accepted answer to Q4"
Enter
Type `grep '"id": 4' key.json swapped-key.json`
Enter
Wait
Sleep 3s
Type "quaestor-cli prove --key swapped-key.json --salt $SALT --sheet sheet.json --out swapped.bin"
Enter
Sleep 1.5s
Hide
Wait
Show
Sleep 3s
Type "# A valid proof, and the score went up to 100%. But the student checks it against the published key:"
Enter
Type "quaestor-cli verify --proof swapped.bin --commitment $C --sheet sheet.json"
Enter
Wait
Sleep 4.5s
Hide
Type "true"
Enter
Wait
Ctrl+L
Sleep 300ms
Show

# ── batch ────────────────────────────────────────────────────────────────
Type "# 4) A whole exam sitting: 3 candidates, ONE proof, one Merkle root"
Enter
Type "ls sheets/"
Enter
Wait
Sleep 1.5s
Type "quaestor-cli prove-batch --key key.json --salt $SALT --sheets sheets/ --out batch.bin --manifest sitting.json"
Enter
Wait
Sleep 4.5s
Ctrl+L
Type "# Each candidate checks their own row in the published results list (sitting.json):"
Enter
Type "quaestor-cli verify-batch --proof batch.bin --manifest sitting.json --commitment $C --sheet sheet.json"
Enter
Wait
Sleep 5s
Ctrl+L

# ── forgery 3 ────────────────────────────────────────────────────────────
Type "# Forgery 3: after proving, the school raises the 60% candidate to 100% in the published list"
Enter
Type `sed 's/"score_bp": 6000/"score_bp": 10000/' sitting.json > tampered.json`
Enter
Wait
Type "grep score_bp tampered.json"
Enter
Wait
Sleep 2.5s
Type "quaestor-cli verify-batch --proof batch.bin --manifest tampered.json --commitment $C --sheet other-sheet.json"
Enter
Wait
Sleep 5s
Hide
Type "true"
Enter
Wait
Ctrl+L
Sleep 300ms
Show

# ── outro ────────────────────────────────────────────────────────────────
Type "# Honest grades verify. Every forgery is rejected."
Enter
Sleep 1s
Type "# github.com/kadircanyildirm-crypto/quaestor"
Enter
Sleep 5s
