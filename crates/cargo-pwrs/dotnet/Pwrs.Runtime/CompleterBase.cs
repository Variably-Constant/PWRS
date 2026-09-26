using System;
using System.Collections;
using System.Collections.Generic;
using System.Collections.ObjectModel;
using System.Management.Automation;
using System.Management.Automation.Language;

namespace Pwrs
{
    /// <summary>
    /// Base of every generated argument completer. The generated
    /// subclass supplies the module and the completer id; this class
    /// forwards to the native completer and shapes the results.
    /// </summary>
    public abstract class CompleterBase : IArgumentCompleter
    {
        protected abstract NativeModule Module { get; }
        protected abstract uint CompleterId { get; }

        public IEnumerable<CompletionResult> CompleteArgument(
            string commandName,
            string parameterName,
            string wordToComplete,
            CommandAst commandAst,
            IDictionary fakeBoundParameters)
        {
            string command = commandAst?.ToString() ?? string.Empty;
            object?[] rows = Module.Complete(CompleterId, wordToComplete ?? string.Empty, command, fakeBoundParameters ?? new Hashtable());
            var results = new Collection<CompletionResult>();
            foreach (object? row in rows)
            {
                string[] cells = ToCells(row);
                if (cells.Length < 4) continue;
                var kind = Enum.TryParse(cells[2], out CompletionResultType k) ? k : CompletionResultType.ParameterValue;
                results.Add(new CompletionResult(cells[0], cells[1], kind, string.IsNullOrEmpty(cells[3]) ? cells[0] : cells[3]));
            }
            return results;
        }

        private static string[] ToCells(object? row)
        {
            object? b = row is PSObject ps ? ps.BaseObject : row;
            if (b is string[] s) return s;
            if (b is object[] o)
            {
                var cells = new string[o.Length];
                for (int i = 0; i < o.Length; i++) cells[i] = o[i]?.ToString() ?? string.Empty;
                return cells;
            }
            return Array.Empty<string>();
        }
    }
}
