# Pester 5 syntax; runs in pwsh and Windows PowerShell. PWRS_MODULE
# points at the built module folder.
#
# The module makes its own TLS connections through rustls. The handshake
# test needs the network and marks itself skipped when the endpoint cannot
# be reached, so a gate without a route out still passes; the others stay
# on this machine.
BeforeAll {
    $module = $env:PWRS_MODULE
    if (-not $module) { throw 'PWRS_MODULE is not set' }
    Import-Module (Join-Path $module 'Tls.psd1') -Force -ErrorAction Stop
    $script:endpoint = 'pq.cloudflareresearch.com'
    $client = New-Object System.Net.Sockets.TcpClient
    try {
        $script:online = $client.ConnectAsync($endpoint, 443).Wait(5000)
    } catch [System.AggregateException] {
        $script:online = $false
    } finally {
        $client.Dispose()
    }
}

Describe 'Get-RustTlsInfo' {
    It 'refuses an address that is not https, by position and from the pipeline' {
        { Get-RustTlsInfo 'http://example.com/' -ErrorAction Stop } | Should -Throw '*not an https:// address*'
        { 'ftp://example.com/' | Get-RustTlsInfo -ErrorAction Stop } | Should -Throw '*not an https:// address*'
    }

    It 'refuses a port that is not a number' {
        { Get-RustTlsInfo 'https://example.com:tls/' -ErrorAction Stop } | Should -Throw '*is not a port*'
    }

    It 'reports a connection nobody accepts as an error' {
        { Get-RustTlsInfo 'https://127.0.0.1:1/' -TimeoutSeconds 5 -ErrorAction Stop } | Should -Throw
    }

    It 'negotiates TLS 1.3 with X25519MLKEM768 where the server offers it' {
        if (-not $online) {
            Set-ItResult -Skipped -Because "$endpoint port 443 cannot be reached from here"
            return
        }
        $r = Get-RustTlsInfo "https://$endpoint/cdn-cgi/trace"
        $r.Protocol | Should -Be 'TLSv1_3'
        $r.KeyExchange | Should -Be 'X25519MLKEM768'
        $r.Status | Should -BeLike 'HTTP/1.? 200*'
        $r.Body | Should -Match 'kex=X25519MLKEM768'
    }
}
