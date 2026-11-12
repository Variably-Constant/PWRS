using System;
using System.Management.Automation;

namespace Pwrs
{
    /// <summary>
    /// Base of the validation attribute a generated shell declares on a
    /// parameter whose Rust field names a validate_with function. The
    /// generated subclass supplies the module; each use names the cmdlet
    /// and the parameter's index in its block.
    ///
    /// The engine calls this after converting the argument to the
    /// parameter's declared type, so the function sees the value the
    /// cmdlet would receive. Its error refuses the argument through a
    /// ValidationMetadataException with the error's message alone, which
    /// the binder reports as a validation failure naming the parameter.
    /// The inner exception is the error's own: of the type the error
    /// names, or a PwrsException for an error that names none, with the
    /// error's id and category in its Data and its details inside it.
    /// </summary>
    [AttributeUsage(AttributeTargets.Property | AttributeTargets.Field)]
    public abstract class ValidateWithBase : ValidateArgumentsAttribute
    {
        private readonly uint _cmdletId;
        private readonly uint _parameter;

        protected ValidateWithBase(int cmdletId, int parameter)
        {
            _cmdletId = (uint)cmdletId;
            _parameter = (uint)parameter;
        }

        protected abstract NativeModule Module { get; }

        protected override void Validate(object arguments, EngineIntrinsics engineIntrinsics)
        {
            try
            {
                Module.Validate(_cmdletId, _parameter, arguments);
            }
            catch (Exception e) when (NamedException.TryRecordOf(e, out _, out _))
            {
                throw new ValidationMetadataException(e.Message, e);
            }
            catch (PwrsException e)
            {
                // A panic, or a library that cannot run the function: no
                // error of the function's, so no id to keep.
                throw new ValidationMetadataException(e.Message, e.InnerException);
            }
        }
    }
}
