using System;
using System.Collections.Generic;
using System.Management.Automation;

namespace Pwrs
{
    /// <summary>
    /// The shell's ValidateScript. PowerShell's own takes a ScriptBlock,
    /// which an attribute argument cannot be, so this one takes the
    /// script's text. For each element of the argument it runs the script
    /// in a scope of its own with the element as <c>$_</c>. An element
    /// the script does not return a true value for is refused with the
    /// message PowerShell's ValidateScript gives, which the binder reports
    /// as a validation failure naming the parameter.
    /// </summary>
    [AttributeUsage(AttributeTargets.Property | AttributeTargets.Field)]
    public sealed class ValidateScriptTextAttribute : ValidateEnumeratedArgumentsAttribute
    {
        public ValidateScriptTextAttribute(string script)
        {
            Script = script;
        }

        /// <summary>The script's text, as the parameter declares it.</summary>
        public string Script { get; }

        protected override void ValidateElement(object element)
        {
            var dollarUnder = new List<PSVariable> { new PSVariable("_", element) };
            var results = ScriptBlock.Create(Script).InvokeWithContext(null, dollarUnder);
            object? result = results.Count switch
            {
                0 => null,
                1 => results[0]?.BaseObject,
                _ => results,
            };
            if (!LanguagePrimitives.IsTrue(result))
            {
                throw new ValidationMetadataException(string.Format(
                    "The \"{1}\" validation script for the argument with value \"{0}\" did not return a result of True. Determine why the validation script failed, and then try the command again.",
                    element,
                    Script));
            }
        }
    }
}
