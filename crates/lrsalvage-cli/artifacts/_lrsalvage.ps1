
using namespace System.Management.Automation
using namespace System.Management.Automation.Language

Register-ArgumentCompleter -Native -CommandName 'lrsalvage' -ScriptBlock {
    param($wordToComplete, $commandAst, $cursorPosition)

    $commandElements = $commandAst.CommandElements
    $command = @(
        'lrsalvage'
        for ($i = 1; $i -lt $commandElements.Count; $i++) {
            $element = $commandElements[$i]
            if ($element -isnot [StringConstantExpressionAst] -or
                $element.StringConstantType -ne [StringConstantType]::BareWord -or
                $element.Value.StartsWith('-') -or
                $element.Value -eq $wordToComplete) {
                break
        }
        $element.Value
    }) -join ';'

    $completions = @(switch ($command) {
        'lrsalvage' {
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help (see more with ''--help'')')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help (see more with ''--help'')')
            [CompletionResult]::new('-V', '-V ', [CompletionResultType]::ParameterName, 'Print version')
            [CompletionResult]::new('--version', '--version', [CompletionResultType]::ParameterName, 'Print version')
            [CompletionResult]::new('scan', 'scan', [CompletionResultType]::ParameterValue, 'Report what is recoverable. Writes nothing except the files you name')
            [CompletionResult]::new('extract', 'extract', [CompletionResultType]::ParameterValue, 'Write the recoverable JPEGs, plus a report and a JSON manifest')
            [CompletionResult]::new('masks', 'masks', [CompletionResultType]::ParameterValue, 'Decode `.lrcat-data` AI mask layers to PNG')
            [CompletionResult]::new('help', 'help', [CompletionResultType]::ParameterValue, 'Print this message or the help of the given subcommand(s)')
            break
        }
        'lrsalvage;scan' {
            [CompletionResult]::new('--catalog', '--catalog', [CompletionResultType]::ParameterName, 'Catalog file (`*.lrcat`)')
            [CompletionResult]::new('--previews', '--previews', [CompletionResultType]::ParameterName, 'Preview cache directory (`* Previews.lrdata`)')
            [CompletionResult]::new('--contact-sheet', '--contact-sheet', [CompletionResultType]::ParameterName, 'Write a self-contained HTML contact sheet of the best previews')
            [CompletionResult]::new('--report', '--report', [CompletionResultType]::ParameterName, 'Write the path-free verification report to this file')
            [CompletionResult]::new('--min-long-edge', '--min-long-edge', [CompletionResultType]::ParameterName, 'Ignore previews below this long edge')
            [CompletionResult]::new('--max-long-edge', '--max-long-edge', [CompletionResultType]::ParameterName, 'Write the largest preview at or below this long edge, not the largest present. `0` disables the cap')
            [CompletionResult]::new('--json', '--json', [CompletionResultType]::ParameterName, 'Emit a machine-readable summary instead of a table')
            [CompletionResult]::new('--verbose', '--verbose', [CompletionResultType]::ParameterName, 'Print one line per plan item instead of only totals')
            [CompletionResult]::new('--all-levels', '--all-levels', [CompletionResultType]::ParameterName, 'Inspect every level, not only the best one')
            [CompletionResult]::new('--include-present', '--include-present', [CompletionResultType]::ParameterName, 'Also include images whose original file is still present')
            [CompletionResult]::new('--verify-source', '--verify-source', [CompletionResultType]::ParameterName, 'Prove the catalog was only read: hash it, copy it, hash the copy, and fail if the two disagree.')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'lrsalvage;extract' {
            [CompletionResult]::new('--catalog', '--catalog', [CompletionResultType]::ParameterName, 'catalog')
            [CompletionResult]::new('--previews', '--previews', [CompletionResultType]::ParameterName, 'previews')
            [CompletionResult]::new('--out', '--out', [CompletionResultType]::ParameterName, 'Destination directory. Must not be inside the catalog or cache')
            [CompletionResult]::new('--min-long-edge', '--min-long-edge', [CompletionResultType]::ParameterName, 'Ignore previews below this long edge')
            [CompletionResult]::new('--max-long-edge', '--max-long-edge', [CompletionResultType]::ParameterName, 'Write the largest preview at or below this long edge, not the largest present. `0` disables the cap')
            [CompletionResult]::new('--dry-run', '--dry-run', [CompletionResultType]::ParameterName, 'Do everything except write files')
            [CompletionResult]::new('--all-levels', '--all-levels', [CompletionResultType]::ParameterName, 'all-levels')
            [CompletionResult]::new('--include-present', '--include-present', [CompletionResultType]::ParameterName, 'Also include images whose original file is still present')
            [CompletionResult]::new('--verify-source', '--verify-source', [CompletionResultType]::ParameterName, 'Prove the catalog was only read: hash it, copy it, hash the copy, and fail if the two disagree.')
            [CompletionResult]::new('--no-metadata', '--no-metadata', [CompletionResultType]::ParameterName, 'Do not add EXIF, XMP rating or the ICC profile to the recovered JPEGs')
            [CompletionResult]::new('--no-timestamps', '--no-timestamps', [CompletionResultType]::ParameterName, 'Do not set file modification times to the capture time')
            [CompletionResult]::new('--force', '--force', [CompletionResultType]::ParameterName, 'Replace an existing destination file that is not a verified extraction')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help (see more with ''--help'')')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help (see more with ''--help'')')
            break
        }
        'lrsalvage;masks' {
            [CompletionResult]::new('--data', '--data', [CompletionResultType]::ParameterName, 'A `<catalog>.lrcat-data` directory')
            [CompletionResult]::new('--out', '--out', [CompletionResultType]::ParameterName, 'out')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'lrsalvage;help' {
            [CompletionResult]::new('scan', 'scan', [CompletionResultType]::ParameterValue, 'Report what is recoverable. Writes nothing except the files you name')
            [CompletionResult]::new('extract', 'extract', [CompletionResultType]::ParameterValue, 'Write the recoverable JPEGs, plus a report and a JSON manifest')
            [CompletionResult]::new('masks', 'masks', [CompletionResultType]::ParameterValue, 'Decode `.lrcat-data` AI mask layers to PNG')
            [CompletionResult]::new('help', 'help', [CompletionResultType]::ParameterValue, 'Print this message or the help of the given subcommand(s)')
            break
        }
        'lrsalvage;help;scan' {
            break
        }
        'lrsalvage;help;extract' {
            break
        }
        'lrsalvage;help;masks' {
            break
        }
        'lrsalvage;help;help' {
            break
        }
    })

    $completions.Where{ $_.CompletionText -like "$wordToComplete*" } |
        Sort-Object -Property ListItemText
}
