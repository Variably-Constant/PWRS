# FileFacts as it is built, run in pwsh and in Windows PowerShell.
# cargo pwrs test sets PWRS_MODULE to the module's folder.
BeforeAll {
    Import-Module (Join-Path $env:PWRS_MODULE 'FileFacts.psd1') -Force -ErrorAction Stop
}

Describe 'FileFacts' {
    # Each test starts in Pester's TestDrive folder, with the same two
    # files written fresh.
    BeforeEach {
        Push-Location -LiteralPath $TestDrive
        Set-Content fruit.txt "apple`nbanana`ncherry`n" -NoNewline
        Set-Content veg.txt "kale`nleek`n" -NoNewline
    }

    AfterEach {
        Pop-Location
    }

    Context 'Measure-FileFactsText' {
        It 'counts the lines, words and bytes of a file and how its lines end' {
            $facts = Measure-FileFactsText fruit.txt
            $facts.Name | Should -Be 'fruit.txt'
            $facts.Lines | Should -Be 3
            $facts.Words | Should -Be 3
            $facts.Bytes | Should -Be 20
            $facts.LineEnding | Should -Be 'Lf'
        }

        It 'takes names and files from the pipeline' {
            @('fruit.txt', 'veg.txt' | Measure-FileFactsText).Lines | Should -Be @(3, 2)
            @(Get-Item fruit.txt, veg.txt | Measure-FileFactsText).Lines | Should -Be @(3, 2)
        }

        It 'reports a name that matches no file and measures the rest' {
            $facts = Measure-FileFactsText missing.txt, veg.txt -ErrorVariable problems -ErrorAction SilentlyContinue
            $facts.Name | Should -Be 'veg.txt'
            $problems.Count | Should -Be 1
            $problems[0].FullyQualifiedErrorId | Should -BeLike 'FileNotFound,*'
        }
    }

    Context 'New-FileFactsIndex' {
        It 'says where a word appears, without regard to case' {
            $index = New-FileFactsIndex fruit.txt, veg.txt
            $index.Files | Should -Be 2
            $index.Words | Should -Be 5
            $place = $index.Find('LEEK')
            $place.File | Should -Be 'veg.txt'
            $place.Line | Should -Be 2
        }

        It 'takes names and files from the pipeline' {
            ('fruit.txt' | New-FileFactsIndex).Files | Should -Be 1
            (Get-Item fruit.txt, veg.txt | New-FileFactsIndex).Files | Should -Be 2
        }
    }

    Context 'Find-FileFactsLine' {
        It 'finds the lines a pattern matches, without regard to case' {
            (Find-FileFactsLine fruit.txt -Pattern '^B').Text | Should -Be 'banana'
        }

        It 'tells capitals from small letters under -Case Match' {
            Find-FileFactsLine fruit.txt -Pattern '^B' -Case Match | Should -BeNullOrEmpty
        }

        It 'finds the lines a script block returns true for' {
            (Find-FileFactsLine fruit.txt -FilterScript { param($line) $line.Length -gt 5 }).Text | Should -Be @('banana', 'cherry')
        }

        It 'takes names and files from the pipeline' {
            ('veg.txt' | Find-FileFactsLine -Pattern '^k').Line | Should -Be 1
            (Get-Item veg.txt | Find-FileFactsLine -Pattern 'ee').Text | Should -Be 'leek'
        }
    }

    Context 'Update-FileFactsText' {
        BeforeEach {
            Set-Content notes.txt "teh cat sat`n" -NoNewline
        }

        It 'replaces text in a file' {
            Update-FileFactsText notes.txt -Replace @{ teh = 'the' }
            Get-Content notes.txt -Raw | Should -Be "the cat sat`n"
        }

        It 'changes nothing under -WhatIf' {
            Update-FileFactsText notes.txt -Replace @{ teh = 'the' } -WhatIf
            Get-Content notes.txt -Raw | Should -Be "teh cat sat`n"
        }

        It 'takes -Replace as text' {
            Update-FileFactsText notes.txt -Replace 'teh=the', 'cat=dog'
            Get-Content notes.txt -Raw | Should -Be "the dog sat`n"
        }

        It 'takes names and files from the pipeline' {
            'notes.txt' | Update-FileFactsText -Replace @{ teh = 'the' }
            Get-Item notes.txt | Update-FileFactsText -Replace @{ cat = 'dog' }
            Get-Content notes.txt -Raw | Should -Be "the dog sat`n"
        }
    }

    Context 'Get-FileFactsHash' {
        It 'hashes each file as Get-FileHash does, in the order given' {
            $hashes = Get-FileFactsHash fruit.txt, veg.txt
            $hashes.Name | Should -Be @('fruit.txt', 'veg.txt')
            $hashes[0].Hash | Should -Be (Get-FileHash fruit.txt -Algorithm SHA256).Hash.ToLowerInvariant()
            $hashes[1].Bytes | Should -Be 10
        }

        It 'takes names and files from the pipeline' {
            ('fruit.txt' | Get-FileFactsHash).Bytes | Should -Be 20
            (Get-Item veg.txt | Get-FileFactsHash).Bytes | Should -Be 10
        }
    }

    Context 'Find-FileFactsDuplicate and Remove-FileFactsDuplicate' {
        BeforeEach {
            Set-Content copy.txt "apple`nbanana`ncherry`n" -NoNewline
        }

        It 'groups the files with the same bytes' {
            $groups = @(Get-FileFactsHash fruit.txt, copy.txt, veg.txt | Find-FileFactsDuplicate)
            $groups.Count | Should -Be 1
            $groups[0].Names | Should -Be @('fruit.txt', 'copy.txt')
        }

        It 'removes every copy but the first' {
            Get-FileFactsHash fruit.txt, copy.txt, veg.txt | Find-FileFactsDuplicate | Remove-FileFactsDuplicate -Confirm:$false
            Test-Path copy.txt | Should -BeFalse
            Test-Path fruit.txt | Should -BeTrue
        }

        It 'removes nothing under -WhatIf' {
            Get-FileFactsHash fruit.txt, copy.txt | Find-FileFactsDuplicate | Remove-FileFactsDuplicate -WhatIf
            Test-Path copy.txt | Should -BeTrue
        }
    }

    Context 'Start-FileFactsHash' {
        AfterEach {
            Get-Event | Remove-Event
        }

        It 'raises FileFacts.Hashed with the hashes once they are done' {
            Start-FileFactsHash fruit.txt, veg.txt
            $done = Wait-Event -SourceIdentifier FileFacts.Hashed -Timeout 10
            $done | Should -Not -BeNullOrEmpty
            $done.MessageData.Hashes.Name | Should -Be @('fruit.txt', 'veg.txt')
            $done.MessageData.Errors | Should -BeNullOrEmpty
        }

        It 'takes names and files from the pipeline' {
            'fruit.txt' | Start-FileFactsHash
            (Wait-Event -SourceIdentifier FileFacts.Hashed -Timeout 10).MessageData.Hashes.Name | Should -Be 'fruit.txt'
            Remove-Event -SourceIdentifier FileFacts.Hashed
            Get-Item veg.txt | Start-FileFactsHash
            (Wait-Event -SourceIdentifier FileFacts.Hashed -Timeout 10).MessageData.Hashes.Name | Should -Be 'veg.txt'
        }
    }

    Context 'Show-FileFactsText' {
        It 'writes each file and its line count to the console, the name in color' {
            $shown = @(Show-FileFactsText fruit.txt 6>&1)
            ($shown | ForEach-Object { "$_" }) -join '' | Should -Be 'fruit.txt   3 lines'
            $shown[0].MessageData.ForegroundColor | Should -Be 'Cyan'
        }

        It 'writes nothing to the output' {
            Show-FileFactsText fruit.txt 6>$null | Should -BeNullOrEmpty
        }

        It 'takes names and files from the pipeline' {
            @('veg.txt' | Show-FileFactsText 6>&1).Count | Should -Be 2
            @(Get-Item veg.txt | Show-FileFactsText 6>&1).Count | Should -Be 2
        }
    }

    Context 'Remove-FileFactsEmpty' {
        BeforeEach {
            Set-Content empty.txt '' -NoNewline
        }

        It 'removes empty files and leaves the rest' {
            Remove-FileFactsEmpty fruit.txt, empty.txt
            Test-Path empty.txt | Should -BeFalse
            Test-Path fruit.txt | Should -BeTrue
        }

        It 'removes nothing under -WhatIf' {
            Remove-FileFactsEmpty empty.txt -WhatIf
            Test-Path empty.txt | Should -BeTrue
        }

        It 'takes names and files from the pipeline' {
            'empty.txt' | Remove-FileFactsEmpty -WhatIf
            Get-Item empty.txt | Remove-FileFactsEmpty
            Test-Path empty.txt | Should -BeFalse
        }
    }
}
