using System;
using System.Management.Automation;
using System.Management.Automation.Provider;
using System.Reflection;
using System.Runtime.InteropServices;
using System.Threading;
using System.Transactions;

namespace Pwrs
{
    /// <summary>
    /// The PowerShell transaction a cmdlet or a provider call runs in. A
    /// command takes part in one only when it was given -UseTransaction
    /// inside Start-Transaction. Windows PowerShell gives -UseTransaction
    /// to a cmdlet declaring SupportsTransactions and to the commands on a
    /// provider declaring the Transactions capability; PowerShell 7 gives
    /// it to neither, so there a command never has a transaction.
    /// </summary>
    internal static class Transactions
    {
        /// <summary>
        /// Whether a cmdlet type's [Cmdlet] declares SupportsTransactions,
        /// read from what the shell wrote. PowerShell 7's CmdletAttribute
        /// reads SupportsTransactions false whatever it was given, which is
        /// how it gives no cmdlet -UseTransaction, so the attribute object
        /// cannot tell.
        /// </summary>
        internal static bool Declared(Type cmdlet)
        {
            foreach (CustomAttributeData attribute in cmdlet.GetCustomAttributesData())
            {
                if (attribute.AttributeType != typeof(CmdletAttribute)) continue;
                foreach (CustomAttributeNamedArgument named in attribute.NamedArguments)
                {
                    if (named.MemberName == nameof(CmdletAttribute.SupportsTransactions) && named.TypedValue.Value is bool declared) return declared;
                }
            }
            return false;
        }

        /// <summary>The cmdlet's transaction, or null when it has none.</summary>
        internal static Transaction? Current(Cmdlet cmdlet)
        {
            if (!cmdlet.TransactionAvailable()) return null;
#if NET
            using (cmdlet.CurrentPSTransaction) return Transaction.Current;
#else
            if (CmdletTransaction == null) throw new PwrsException("this PowerShell's Cmdlet has no CurrentPSTransaction, so a cmdlet cannot read its transaction");
            using ((IDisposable?)CmdletTransaction.GetValue(cmdlet, null)) return Transaction.Current;
#endif
        }

#if !NET
        /// <summary>
        /// Cmdlet.CurrentPSTransaction, which Windows PowerShell has and the
        /// PowerShellStandard reference this target compiles against leaves
        /// out.
        /// </summary>
        private static readonly PropertyInfo? CmdletTransaction = typeof(Cmdlet).GetProperty("CurrentPSTransaction", BindingFlags.Public | BindingFlags.Instance);
#endif

        /// <summary>The provider call's transaction, or null when it has none.</summary>
        internal static Transaction? Current(CmdletProvider provider)
        {
            if (!provider.TransactionAvailable()) return null;
            using (provider.CurrentPSTransaction) return Transaction.Current;
        }
    }

    /// <summary>
    /// A Rust resource enlisted in a transaction as a volatile enlistment.
    /// Its callbacks run on the thread that completes or rolls back the
    /// transaction: the pipeline thread for Complete-Transaction and
    /// Undo-Transaction, a timer's for a timeout. Prepare calls the native
    /// side to check whether the resource can commit, and a refusal rolls the
    /// transaction back with the native error. The native end runs once,
    /// with the outcome, and frees the context; it runs after a refused
    /// prepare too, which System.Transactions follows with no Rollback of
    /// its own.
    /// </summary>
    internal sealed unsafe class RustEnlistment : IEnlistmentNotification
    {
#if NET
        private readonly delegate* unmanaged[Cdecl]<IntPtr, IntPtr*, int> _prepare;
        private readonly delegate* unmanaged[Cdecl]<IntPtr, byte, void> _end;
#else
        [UnmanagedFunctionPointer(CallingConvention.Cdecl)] private delegate int PrepareFn(IntPtr context, IntPtr* err);
        [UnmanagedFunctionPointer(CallingConvention.Cdecl)] private delegate void EndFn(IntPtr context, byte committed);
        private readonly PrepareFn _prepare;
        private readonly EndFn _end;
#endif
        /// <summary>The native context, until the end takes it.</summary>
        private IntPtr _context;

        /// <summary>
        /// An enlistment for the native callbacks. It owns the context once
        /// it is enlisted; until then the caller does, so a transaction
        /// that refuses it leaves the context with the caller.
        /// </summary>
        internal RustEnlistment(IntPtr prepare, IntPtr end, IntPtr context)
        {
#if NET
            _prepare = (delegate* unmanaged[Cdecl]<IntPtr, IntPtr*, int>)prepare;
            _end = (delegate* unmanaged[Cdecl]<IntPtr, byte, void>)end;
#else
            _prepare = Marshal.GetDelegateForFunctionPointer<PrepareFn>(prepare);
            _end = Marshal.GetDelegateForFunctionPointer<EndFn>(end);
#endif
            _context = context;
        }

        public void Prepare(PreparingEnlistment preparingEnlistment)
        {
            IntPtr err = IntPtr.Zero;
            int status = _prepare(Volatile.Read(ref _context), &err);
            object? errObj = Native.TakeErr(err);
            if (status == Native.Ok)
            {
                preparingEnlistment.Prepared();
                return;
            }
            Exception refusal = errObj is object?[] cells
                ? PwrsException.FromCells(cells, null)
                : errObj as Exception ?? new PwrsException(errObj as string ?? $"a Rust resource refused to commit with status {status}");
            End(false);
            preparingEnlistment.ForceRollback(refusal);
        }

        public void Commit(Enlistment enlistment)
        {
            End(true);
            enlistment.Done();
        }

        public void Rollback(Enlistment enlistment)
        {
            End(false);
            enlistment.Done();
        }

        /// <summary>
        /// An outcome the transaction manager cannot tell, which a PowerShell
        /// transaction with no durable resource never has, ends the Rust
        /// resource as a rollback does.
        /// </summary>
        public void InDoubt(Enlistment enlistment)
        {
            End(false);
            enlistment.Done();
        }

        private void End(bool committed)
        {
            IntPtr context = Interlocked.Exchange(ref _context, IntPtr.Zero);
            if (context != IntPtr.Zero) _end(context, (byte)(committed ? 1 : 0));
        }
    }
}
