#compdef -P _zub
# Shared zub completer. The program name comes from $service (the name this
# function was registered to complete); it is never hardcoded.
_zub() {
  local prog="$service"
  local context state state_descr line
  local ret=1
  local -a list
  local comps has

  _arguments -C \
             '1: :->cmds' \
             '*:: :->args' && ret=0

  export COMP_WORD="${line[-1]}"

  case $state in
    cmds)
      comps="$(_call_program ${prog}-cmds $prog completions --commands)"
      ;;
    args)
      subcommand="$line[1]"
      comps="$(_call_program ${prog}-${subcommand}-args $prog completions "${line[1,-2]}")"
      has="$?"
      ;;
  esac

  unset COMP_WORD

  if [[ -z "$comps" ]]; then
    if [[ "$has" = 42 ]]; then
      _default
    else
      ret=1
      _message "No completions"
    fi
  else
    ret=0
    list=("${(ps:\n:)comps}")
    _values "${prog}-command" "${list[@]}"
  fi

  return ret
}

_zub "$@"
