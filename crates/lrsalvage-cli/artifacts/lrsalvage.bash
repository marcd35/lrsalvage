_lrsalvage() {
    local i cur prev opts cmd
    COMPREPLY=()
    if [[ "${BASH_VERSINFO[0]}" -ge 4 ]]; then
        cur="$2"
    else
        cur="${COMP_WORDS[COMP_CWORD]}"
    fi
    prev="$3"
    cmd=""
    opts=""

    for i in "${COMP_WORDS[@]:0:COMP_CWORD}"
    do
        case "${cmd},${i}" in
            ",$1")
                cmd="lrsalvage"
                ;;
            lrsalvage,extract)
                cmd="lrsalvage__subcmd__extract"
                ;;
            lrsalvage,help)
                cmd="lrsalvage__subcmd__help"
                ;;
            lrsalvage,masks)
                cmd="lrsalvage__subcmd__masks"
                ;;
            lrsalvage,scan)
                cmd="lrsalvage__subcmd__scan"
                ;;
            lrsalvage__subcmd__help,extract)
                cmd="lrsalvage__subcmd__help__subcmd__extract"
                ;;
            lrsalvage__subcmd__help,help)
                cmd="lrsalvage__subcmd__help__subcmd__help"
                ;;
            lrsalvage__subcmd__help,masks)
                cmd="lrsalvage__subcmd__help__subcmd__masks"
                ;;
            lrsalvage__subcmd__help,scan)
                cmd="lrsalvage__subcmd__help__subcmd__scan"
                ;;
            *)
                ;;
        esac
    done

    case "${cmd}" in
        lrsalvage)
            opts="-h -V --help --version scan extract masks help"
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 1 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        lrsalvage__subcmd__extract)
            opts="-h --catalog --previews --out --dry-run --min-long-edge --max-long-edge --all-levels --include-present --verify-source --no-metadata --no-timestamps --force --help"
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 2 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                --catalog)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --previews)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --out)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --min-long-edge)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --max-long-edge)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        lrsalvage__subcmd__help)
            opts="scan extract masks help"
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 2 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        lrsalvage__subcmd__help__subcmd__extract)
            opts=""
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 3 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        lrsalvage__subcmd__help__subcmd__help)
            opts=""
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 3 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        lrsalvage__subcmd__help__subcmd__masks)
            opts=""
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 3 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        lrsalvage__subcmd__help__subcmd__scan)
            opts=""
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 3 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        lrsalvage__subcmd__masks)
            opts="-h --data --out --help"
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 2 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                --data)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --out)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        lrsalvage__subcmd__scan)
            opts="-h --catalog --previews --json --contact-sheet --report --verbose --min-long-edge --max-long-edge --all-levels --include-present --verify-source --help"
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 2 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                --catalog)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --previews)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --contact-sheet)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --report)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --min-long-edge)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --max-long-edge)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
    esac
}

if [[ "${BASH_VERSINFO[0]}" -eq 4 && "${BASH_VERSINFO[1]}" -ge 4 || "${BASH_VERSINFO[0]}" -gt 4 ]]; then
    complete -F _lrsalvage -o nosort -o bashdefault -o default lrsalvage
else
    complete -F _lrsalvage -o bashdefault -o default lrsalvage
fi
