# Pester 4+ syntax; runs in pwsh and Windows PowerShell. PWRS_MODULE
# points at the built module folder.
#
# A proxy class that names a view method is shown by the text the method
# returns, written as it is, and still shows its properties through
# Format-Table and Format-List. A class that names its columns is shown
# by a table of them, and Format-List still shows every property.
BeforeAll {
    $module = $env:PWRS_MODULE
    if (-not $module) { throw 'PWRS_MODULE is not set' }
    Import-Module (Join-Path $module 'Hello.psd1') -Force -ErrorAction Stop

    # Output text as lines, whatever line ending the host wrote, with the
    # blank lines the formatter adds around a view left out.
    function Get-Lines([string] $Text) {
        @($Text -split "`r?`n" | Where-Object { $_.Trim() })
    }

    # The same, with any SGR escape sequence a table header carries left
    # out.
    function Get-PlainLines([string] $Text) {
        Get-Lines ($Text -replace "$([char]27)\[[0-9;]*m", '')
    }
}

Describe 'A class that names a view method' {
    It 'is shown by the text the method returns' {
        $frame = New-RustFrame 3 1
        Get-Lines ($frame | Out-String) | Should -Be @('+---+', '|   |', '+---+')
    }

    It 'shows exactly what the method returns' {
        $frame = New-RustFrame 5 2
        (Get-Lines ($frame | Out-String)) -join "`n" | Should -Be $frame.Draw()
    }

    It 'still shows its properties through Format-Table and Format-List' {
        $table = New-RustFrame 3 1 | Format-Table | Out-String
        $table | Should -Match 'Width'
        $table | Should -Match 'Height'
        $table | Should -Not -Match '\+---\+'
        New-RustFrame 3 1 | Format-List | Out-String | Should -Match 'Width\s*:\s*3'
    }

    It 'writes the text as it is, escape sequences included' {
        $rendering = $null
        if (Get-Variable -Name PSStyle -ErrorAction SilentlyContinue) {
            $rendering = $PSStyle.OutputRendering
            $PSStyle.OutputRendering = 'Ansi'
        }
        try {
            $text = New-RustFrame 1 1 -Color | Out-String
        } finally {
            if ($null -ne $rendering) { $PSStyle.OutputRendering = $rendering }
        }
        $text | Should -Match ([regex]::Escape("$([char]27)[32m+-+"))
    }
}

Describe 'A copied class that declares the text it shows' {
    It 'answers ToString with that text' {
        (New-RustSegment 1 2 3 4).Start.ToString() | Should -Be '(1, 2)'
        "$((New-RustSegment 1 2 3 -4).End)" | Should -Be '(3, -4)'
    }

    It 'prints as that text inside another object' {
        New-RustSegment 1 2 3 4 | Format-List | Out-String | Should -Match 'Start\s*:\s*\(1, 2\)'
        New-RustSegment 1 2 3 4 | Format-List | Out-String | Should -Match 'End\s*:\s*\(3, 4\)'
    }

    It 'keeps its properties' {
        $point = (New-RustSegment 1 2 3 4).End
        $point.X | Should -Be 3
        $point.Y | Should -Be 4
    }

    It 'shows a string property, and a null one as nothing' {
        $light = New-RustLight -Name corner -State Green
        "$light" | Should -Be 'corner: Green'
        $light.Name = $null
        $light.ToString() | Should -Be ': Green'
    }
}

Describe 'A class that names its table columns' {
    It 'is shown by a table of exactly those columns' {
        $lines = Get-PlainLines (New-RustJob nightly 10 -Failed 2 | Out-String)
        $lines.Count | Should -Be 3
        $lines[0] | Should -Match '^Name\s+Done\s+Failed\s*$'
        $lines[1] | Should -Match '^-+\s+-+\s+-+\s*$'
        $lines[2] | Should -Match '^nightly\s+8\s+2\s*$'
    }

    It 'is what Format-Table shows' {
        $lines = Get-PlainLines (New-RustJob nightly 10 -Failed 2 | Format-Table | Out-String)
        $lines[0] | Should -Match '^Name\s+Done\s+Failed\s*$'
    }

    It 'still shows every property through Format-List' {
        $list = New-RustJob nightly 10 -Failed 2 | Format-List | Out-String
        foreach ($pair in @('Name\s*:\s*nightly', 'Items\s*:\s*10', 'Done\s*:\s*8', 'Failed\s*:\s*2', 'ElapsedMs\s*:\s*30', 'Workers\s*:\s*4', 'Queue\s*:\s*default')) {
            $list | Should -Match $pair
        }
    }

    It 'names its views Hello.Job.Columns and Hello.Job' {
        $lines = Get-PlainLines (New-RustJob nightly 10 | Format-Table -View Hello.Job.Columns | Out-String)
        $lines[0] | Should -Match '^Name\s+Done\s+Failed\s*$'
        New-RustJob nightly 10 | Format-List -View Hello.Job | Out-String | Should -Match 'Queue\s*:\s*default'
    }
}
