# Finds each command a module wraps in the host running this, pwsh or
# Windows PowerShell, and writes as JSON, by the name the request gave it,
# what the build needs to record a copy of it: the module it comes from and
# that module's version, and either the file of a cmdlet's assembly, which
# the build reads as it reads PowerShell's own, or a function's
# declarations from its param block: each parameter's type and each
# attribute with its constant arguments, every type by full name and
# assembly for the build to complete. An argument that is not a constant,
# such as a script block, is written as the attribute's error. The request:
#
#   {"names": ["Compress-Archive", "Some.Module\\Get-Thing"], "version_only": false}
#
# With version_only, each record has only the module and version.
# Uses only what Windows PowerShell 5.1 runs.
param(
    [Parameter(Mandatory)] [string] $Request,
    [Parameter(Mandatory)] [string] $Output
)
$ErrorActionPreference = 'Stop'

function Get-TypeRef([type] $Type) {
    [ordered]@{ name = $Type.FullName; assembly = $Type.Assembly.GetName().Name }
}

function Get-Value($Value, [type] $Type) {
    if ($null -eq $Value) { return [ordered]@{ kind = 'null'; type = (Get-TypeRef $Type) } }
    if ($Type.IsArray) {
        $element = $Type.GetElementType()
        $items = @(foreach ($v in @($Value)) { Get-Value $v $element })
        return [ordered]@{ kind = 'array'; element = (Get-TypeRef $element); items = $items }
    }
    if ($Type -eq [type]) { return [ordered]@{ kind = 'type'; type = (Get-TypeRef ([type]$Value)) } }
    if ($Type.IsEnum) {
        $number = [System.Convert]::ToInt64(([System.Enum]::Parse($Type, [string]$Value)), [System.Globalization.CultureInfo]::InvariantCulture)
        return [ordered]@{ kind = 'enum'; type = (Get-TypeRef $Type); text = [string]$number }
    }
    if ($Type -eq [bool]) { return [ordered]@{ kind = 'bool'; value = [bool]$Value } }
    if ($Type -eq [string]) { return [ordered]@{ kind = 'string'; value = [string]$Value } }
    if ($Type -eq [char]) { return [ordered]@{ kind = 'char'; value = [string]$Value } }
    if ($Type -eq [object]) {
        if ($Value -is [string]) { return [ordered]@{ kind = 'string'; value = $Value } }
        if ($Value -is [bool]) { return [ordered]@{ kind = 'bool'; value = $Value } }
        $Type = $Value.GetType()
    }
    $converted = [System.Convert]::ChangeType($Value, $Type, [System.Globalization.CultureInfo]::InvariantCulture)
    [ordered]@{ kind = 'number'; type = $Type.FullName; text = [System.Convert]::ToString($converted, [System.Globalization.CultureInfo]::InvariantCulture) }
}

# The constant in an argument's expression, or a throw naming why not.
function Get-Constant([System.Management.Automation.Language.ExpressionAst] $Expression) {
    if ($Expression -is [System.Management.Automation.Language.ScriptBlockExpressionAst]) { throw "a script block argument" }
    if ($Expression -is [System.Management.Automation.Language.TypeExpressionAst]) { return $Expression.TypeName.GetReflectionType() }
    $Expression.SafeGetValue()
}

function Get-Attribute([System.Management.Automation.Language.AttributeAst] $Ast) {
    $type = $Ast.TypeName.GetReflectionAttributeType()
    if ($null -eq $type) { $type = $Ast.TypeName.GetReflectionType() }
    if ($null -eq $type) {
        return [ordered]@{ type = [ordered]@{ name = $Ast.TypeName.FullName; assembly = '' }; error = 'the type is not one this host resolves' }
    }
    $out = [ordered]@{ type = (Get-TypeRef $type); args = @(); named = @() }
    try {
        $positional = @($Ast.PositionalArguments)
        if ($positional.Count -gt 0) {
            $ctor = @($type.GetConstructors() | Where-Object {
                $ps = $_.GetParameters()
                $ps.Count -eq $positional.Count -or ($ps.Count -ge 1 -and $ps[$ps.Count - 1].ParameterType.IsArray -and $ps.Count -le $positional.Count + 1)
            }) | Select-Object -First 1
            if ($null -eq $ctor) { throw "no constructor takes $($positional.Count) arguments" }
            $ps = $ctor.GetParameters()
            if ($ps.Count -eq 1 -and $ps[0].ParameterType.IsArray) {
                $values = @(foreach ($p in $positional) { Get-Constant $p })
                $out.args = @(Get-Value $values $ps[0].ParameterType)
            }
            else {
                $out.args = @(for ($i = 0; $i -lt $positional.Count; $i++) { Get-Value (Get-Constant $positional[$i]) $ps[$i].ParameterType })
            }
        }
        # A script names a member in any case; the attribute declares one.
        $flags = [System.Reflection.BindingFlags]'Public,Instance,IgnoreCase'
        $out.named = @(foreach ($n in $Ast.NamedArguments) {
            $property = $type.GetProperty($n.ArgumentName, $flags)
            $field = if ($property) { $null } else { $type.GetField($n.ArgumentName, $flags) }
            if (-not $property -and -not $field) { throw "$($type.Name) has no member $($n.ArgumentName)" }
            $memberName = if ($property) { $property.Name } else { $field.Name }
            $memberType = if ($property) { $property.PropertyType } else { $field.FieldType }
            $value = if ($n.ExpressionOmitted) { $true } else { Get-Constant $n.Argument }
            [ordered]@{ name = $memberName; value = (Get-Value $value $memberType) }
        })
    }
    catch {
        $out.Remove('args'); $out.Remove('named')
        $out.error = $_.Exception.Message
    }
    $out
}

function Find-Command([string] $Name) {
    $found = Get-Command $Name -ErrorAction SilentlyContinue
    while ($found -is [System.Management.Automation.AliasInfo]) { $found = $found.ResolvedCommand }
    $found
}

function Read-Command([string] $Name, [bool] $VersionOnly) {
    $command = Find-Command $Name
    # A command found by module analysis, before its module is imported,
    # reports that module without its version.
    if ($command -and $command.ModuleName -and ($null -eq $command.Module -or $command.Module.Version -eq [version]'0.0')) {
        Import-Module $command.ModuleName -ErrorAction SilentlyContinue
        $command = Find-Command $Name
    }
    $result = [ordered]@{ found = $false }
    if (-not $command) { return $result }
    $result.found = $true
    $result.module = [string]$command.ModuleName
    # The command's own module can be the script its manifest loads, whose
    # version is 0.0; the imported module of that name has the manifest's.
    # A command of no module, such as one a profile defines, has neither.
    $imported = if ($command.ModuleName) { Get-Module -Name $command.ModuleName | Sort-Object Version -Descending | Select-Object -First 1 }
    $result.version = if ($imported) { [string]$imported.Version } elseif ($command.Module) { [string]$command.Module.Version } else { '' }
    if ($VersionOnly) { return $result }
    if ($command -is [System.Management.Automation.CmdletInfo]) {
        $result.kind = 'cmdlet'
        $result.assembly = $command.ImplementingType.Assembly.Location
        return $result
    }
    $result.kind = 'function'
    $body = $command.ScriptBlock.Ast
    if ($body -is [System.Management.Automation.Language.FunctionDefinitionAst]) { $body = $body.Body }
    $block = $body.ParamBlock
    $result.dynamic = $null -ne $body.DynamicParamBlock
    $binding = @($block.Attributes | Where-Object { $_.TypeName.GetReflectionAttributeType() -eq [System.Management.Automation.CmdletBindingAttribute] }) | Select-Object -First 1
    $result.cmdlet = @(if ($binding) {
        $a = Get-Attribute $binding
        if ($a.named) { $a.named = @($a.named | Where-Object { $_.name -ne 'PositionalBinding' }) }
        $a
    })
    $result.output = @(foreach ($a in $block.Attributes) { if ($a.TypeName.GetReflectionAttributeType() -eq [System.Management.Automation.OutputTypeAttribute]) { Get-Attribute $a } })
    $result.parameters = @(foreach ($p in ([System.Management.Automation.CommandMetadata]::new($command)).Parameters.Values) {
        $ast = @($block.Parameters | Where-Object { $_.Name.VariablePath.UserPath -eq $p.Name }) | Select-Object -First 1
        $attributes = @(if ($ast) { foreach ($a in $ast.Attributes) { if ($a -is [System.Management.Automation.Language.AttributeAst]) { Get-Attribute $a } } })
        # An advanced function's parameter needs no [Parameter]; a
        # compiled one is a parameter only with one.
        if (-not ($attributes | Where-Object { $_.type.name -eq 'System.Management.Automation.ParameterAttribute' })) {
            $attributes = @([ordered]@{ type = (Get-TypeRef ([System.Management.Automation.ParameterAttribute])); args = @(); named = @() }) + $attributes
        }
        [ordered]@{ name = $p.Name; type = (Get-TypeRef $p.ParameterType); attributes = $attributes }
    })
    $result
}

$asked = [System.IO.File]::ReadAllText($Request) | ConvertFrom-Json
$records = [ordered]@{}
foreach ($name in @($asked.names)) { $records[$name] = Read-Command $name ([bool]$asked.version_only) }
$json = $records | ConvertTo-Json -Depth 24
[System.IO.File]::WriteAllText($Output, $json, (New-Object System.Text.UTF8Encoding $false))
