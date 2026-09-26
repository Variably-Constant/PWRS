# Pester 4+ syntax; runs in pwsh and Windows PowerShell. PWRS_MODULE
# points at the built module folder.
#
# Hello.Stretch is a copied class that declares `new` and `parse`.
# Hello.Person is a copied class that declares nothing, and stays the
# check that a class which asks for no constructor keeps the public
# parameterless one it always had.
BeforeAll {
    $module = $env:PWRS_MODULE
    if (-not $module) { throw 'PWRS_MODULE is not set' }
    Import-Module (Join-Path $module 'Hello.psd1') -Force -ErrorAction Stop
}

Describe 'a constructor on a copied class' {
    It 'makes the object Rust built, as the class and not a wrapper' {
        $s = [Hello.Stretch]::new(10, 30)
        $s.GetType().FullName | Should -Be 'Hello.Stretch'
        $s.Start | Should -Be 10
        $s.Length | Should -Be 30
    }

    It 'starts from the Rust Default when every argument is left out, not from CLR zeros' {
        $s = [Hello.Stretch]::new()
        $s.Start | Should -Be 0
        $s.Length | Should -Be 60
    }

    It 'takes the default for an argument left out and the value for one given' {
        $s = [Hello.Stretch]::new(5)
        $s.Start | Should -Be 5
        $s.Length | Should -Be 60
    }

    It 'refuses as an exception, since a constructor has no stream to warn on' {
        $message = ''
        try { [Hello.Stretch]::new(0, -1) } catch { $message = $_.Exception.Message }
        $message | Should -Match 'cannot run -1'
    }

    It 'makes an object whose properties are plain fields a script can change' {
        $s = [Hello.Stretch]::new(1, 2)
        $s.Length = 9
        $s.Length | Should -Be 9
    }
}

Describe 'the constructors a script can reach' {
    It 'offers only the constructor Rust declared once a copied class declares one' {
        $public = [Hello.Stretch].GetConstructors()
        $public.Count | Should -Be 1
        $public[0].GetParameters().Count | Should -Be 2
    }

    It 'keeps the public parameterless constructor on a copied class that declares none' {
        $public = [Hello.Person].GetConstructors()
        @($public | Where-Object { $_.GetParameters().Count -eq 0 }).Count | Should -Be 1
        [Hello.Person]::new().Age | Should -Be 0
    }
}

Describe 'a static on a copied class' {
    It 'runs on the type and returns the class, built through its factory' {
        $s = [Hello.Stretch]::Parse('7+3')
        $s.GetType().FullName | Should -Be 'Hello.Stretch'
        $s.Start | Should -Be 7
        $s.Length | Should -Be 3
    }

    It 'carries the Rust error through, naming what failed' {
        { [Hello.Stretch]::Parse('seven+3') } | Should -Throw -ExpectedMessage '*not a number*'
    }
}
