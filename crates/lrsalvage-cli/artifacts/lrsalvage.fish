# Print an optspec for argparse to handle cmd's options that are independent of any subcommand.
function __fish_lrsalvage_global_optspecs
    string join \n h/help V/version
end

function __fish_lrsalvage_needs_command
    # Figure out if the current invocation already has a command.
    set -l cmd (commandline -opc)
    set -e cmd[1]
    argparse -s (__fish_lrsalvage_global_optspecs) -- $cmd 2>/dev/null
    or return
    if set -q argv[1]
        # Also print the command, so this can be used to figure out what it is.
        echo $argv[1]
        return 1
    end
    return 0
end

function __fish_lrsalvage_using_subcommand
    set -l cmd (__fish_lrsalvage_needs_command)
    test -z "$cmd"
    and return 1
    contains -- $cmd[1] $argv
end

complete -c lrsalvage -n "__fish_lrsalvage_needs_command" -s h -l help -d 'Print help (see more with \'--help\')'
complete -c lrsalvage -n "__fish_lrsalvage_needs_command" -s V -l version -d 'Print version'
complete -c lrsalvage -n "__fish_lrsalvage_needs_command" -f -a "scan" -d 'Report what is recoverable. Writes nothing except the files you name'
complete -c lrsalvage -n "__fish_lrsalvage_needs_command" -f -a "extract" -d 'Write the recoverable JPEGs, plus a report and a JSON manifest'
complete -c lrsalvage -n "__fish_lrsalvage_needs_command" -f -a "masks" -d 'Decode `.lrcat-data` AI mask layers to PNG'
complete -c lrsalvage -n "__fish_lrsalvage_needs_command" -f -a "help" -d 'Print this message or the help of the given subcommand(s)'
complete -c lrsalvage -n "__fish_lrsalvage_using_subcommand scan" -l catalog -d 'Catalog file (`*.lrcat`)' -r -F
complete -c lrsalvage -n "__fish_lrsalvage_using_subcommand scan" -l previews -d 'Preview cache directory (`* Previews.lrdata`)' -r -F
complete -c lrsalvage -n "__fish_lrsalvage_using_subcommand scan" -l contact-sheet -d 'Write a self-contained HTML contact sheet of the best previews' -r -F
complete -c lrsalvage -n "__fish_lrsalvage_using_subcommand scan" -l report -d 'Write the path-free verification report to this file' -r -F
complete -c lrsalvage -n "__fish_lrsalvage_using_subcommand scan" -l min-long-edge -d 'Ignore previews below this long edge' -r
complete -c lrsalvage -n "__fish_lrsalvage_using_subcommand scan" -l max-long-edge -d 'Write the largest preview at or below this long edge, not the largest present. `0` disables the cap' -r
complete -c lrsalvage -n "__fish_lrsalvage_using_subcommand scan" -l json -d 'Emit a machine-readable summary instead of a table'
complete -c lrsalvage -n "__fish_lrsalvage_using_subcommand scan" -l verbose -d 'Print one line per plan item instead of only totals'
complete -c lrsalvage -n "__fish_lrsalvage_using_subcommand scan" -l all-levels -d 'Inspect every level, not only the best one'
complete -c lrsalvage -n "__fish_lrsalvage_using_subcommand scan" -l include-present -d 'Also include images whose original file is still present'
complete -c lrsalvage -n "__fish_lrsalvage_using_subcommand scan" -l verify-source -d 'Prove the catalog was only read: hash it, copy it, hash the copy, and fail if the two disagree.'
complete -c lrsalvage -n "__fish_lrsalvage_using_subcommand scan" -s h -l help -d 'Print help'
complete -c lrsalvage -n "__fish_lrsalvage_using_subcommand extract" -l catalog -r -F
complete -c lrsalvage -n "__fish_lrsalvage_using_subcommand extract" -l previews -r -F
complete -c lrsalvage -n "__fish_lrsalvage_using_subcommand extract" -l out -d 'Destination directory. Must not be inside the catalog or cache' -r -F
complete -c lrsalvage -n "__fish_lrsalvage_using_subcommand extract" -l min-long-edge -d 'Ignore previews below this long edge' -r
complete -c lrsalvage -n "__fish_lrsalvage_using_subcommand extract" -l max-long-edge -d 'Write the largest preview at or below this long edge, not the largest present. `0` disables the cap' -r
complete -c lrsalvage -n "__fish_lrsalvage_using_subcommand extract" -l dry-run -d 'Do everything except write files'
complete -c lrsalvage -n "__fish_lrsalvage_using_subcommand extract" -l all-levels
complete -c lrsalvage -n "__fish_lrsalvage_using_subcommand extract" -l include-present -d 'Also include images whose original file is still present'
complete -c lrsalvage -n "__fish_lrsalvage_using_subcommand extract" -l verify-source -d 'Prove the catalog was only read: hash it, copy it, hash the copy, and fail if the two disagree.'
complete -c lrsalvage -n "__fish_lrsalvage_using_subcommand extract" -l no-metadata -d 'Do not add EXIF, XMP rating or the ICC profile to the recovered JPEGs'
complete -c lrsalvage -n "__fish_lrsalvage_using_subcommand extract" -l no-timestamps -d 'Do not set file modification times to the capture time'
complete -c lrsalvage -n "__fish_lrsalvage_using_subcommand extract" -l force -d 'Replace an existing destination file that is not a verified extraction'
complete -c lrsalvage -n "__fish_lrsalvage_using_subcommand extract" -s h -l help -d 'Print help (see more with \'--help\')'
complete -c lrsalvage -n "__fish_lrsalvage_using_subcommand masks" -l data -d 'A `<catalog>.lrcat-data` directory' -r -F
complete -c lrsalvage -n "__fish_lrsalvage_using_subcommand masks" -l out -r -F
complete -c lrsalvage -n "__fish_lrsalvage_using_subcommand masks" -s h -l help -d 'Print help'
complete -c lrsalvage -n "__fish_lrsalvage_using_subcommand help; and not __fish_seen_subcommand_from scan extract masks help" -f -a "scan" -d 'Report what is recoverable. Writes nothing except the files you name'
complete -c lrsalvage -n "__fish_lrsalvage_using_subcommand help; and not __fish_seen_subcommand_from scan extract masks help" -f -a "extract" -d 'Write the recoverable JPEGs, plus a report and a JSON manifest'
complete -c lrsalvage -n "__fish_lrsalvage_using_subcommand help; and not __fish_seen_subcommand_from scan extract masks help" -f -a "masks" -d 'Decode `.lrcat-data` AI mask layers to PNG'
complete -c lrsalvage -n "__fish_lrsalvage_using_subcommand help; and not __fish_seen_subcommand_from scan extract masks help" -f -a "help" -d 'Print this message or the help of the given subcommand(s)'
