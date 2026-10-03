# Where a caller's time goes, for the same number of greetings through
# five shapes.
#
# Separates what the loop construct costs from what invoking the cmdlet
# costs, so the two are not read as one number. The empty rows are the
# construct alone; the difference between an empty row and the row
# below it is the invocation.
#
# Reduced by minimum across rounds: competing work only ever adds time.
param([Parameter(Mandatory)] [string] $Module, [int] $N = 50000)
$ErrorActionPreference = 'Stop'
Import-Module (Join-Path $Module 'Hello.psd1') -Force -ErrorAction Stop

function Measure-Shape([string] $Name, [scriptblock] $Block) {
    & $Block | Out-Null
    $best = [double]::MaxValue
    foreach ($round in 1..5) {
        $sw = [System.Diagnostics.Stopwatch]::StartNew()
        & $Block | Out-Null
        $sw.Stop()
        if ($sw.Elapsed.TotalMilliseconds -lt $best) { $best = $sw.Elapsed.TotalMilliseconds }
    }
    "{0,-42} {1,8:N0} ms   {2,7:N2} us/item" -f $Name, $best, ($best * 1000 / $N)
}

Measure-Shape 'ForEach-Object, empty block'        { 1..$N | ForEach-Object { } }
Measure-Shape 'foreach statement, empty body'      { foreach ($i in 1..$N) { } }
Measure-Shape 'ForEach-Object { Get-Greeting }'    { 1..$N | ForEach-Object { Get-Greeting -Name x } }
Measure-Shape 'foreach statement { Get-Greeting }' { foreach ($i in 1..$N) { Get-Greeting -Name x } }
Measure-Shape 'pipeline, 1..N | Get-Greeting'      { 1..$N | Get-Greeting }
