__ps1() {
  local s=$?
  PS1=''
  if [ "$s" -ne 0 ]; then PS1='\[\e[1;31m\]✗ exit '"$s"'\[\e[0m\] '; fi
  PS1+='\[\e[1;35m\]quaestor\[\e[0m\]:\[\e[1;34m\]~/exam\[\e[0m\]$ '
}
PROMPT_COMMAND=__ps1
export SALT=0101010101010101010101010101010101010101010101010101010101010101
export PATH=/target/release:$PATH
export CARGO_TARGET_DIR=/target
cd /root/exam
