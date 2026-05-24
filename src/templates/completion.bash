_zub_complete() {
  local prog="${COMP_WORDS[0]##*/}"
  COMPREPLY=()
  local word="${COMP_WORDS[COMP_CWORD]}"
  local completions
  if [ "$COMP_CWORD" -eq 1 ]; then
    completions="$("$prog" completions --commands | sed -e 's/\[.*//g')"
  else
    export COMP_WORD="$word"
    completions="$("$prog" completions "${COMP_WORDS[1]}" "${COMP_WORDS[@]:2}" | sed -e 's/\[.*//g')"
    unset COMP_WORD
  fi
  COMPREPLY=( $(compgen -W "$completions" -- "$word") )
}
complete -F _zub_complete @NAME@
