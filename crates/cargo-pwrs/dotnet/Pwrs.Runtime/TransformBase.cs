using System;
using System.Management.Automation;

namespace Pwrs
{
    /// <summary>
    /// Base of every generated argument transformation. The generated
    /// subclass supplies the module and the transform id; this class
    /// forwards the value the binder is about to assign and returns
    /// what to assign instead.
    ///
    /// The engine calls this before converting the argument to the
    /// parameter's declared type and before validation, so a refusal
    /// is a binding failure naming the parameter, which is what an
    /// ArgumentTransformationMetadataException produces. It has the
    /// error's message alone, and its inner exception is the error's own:
    /// of the type the error names, or a PwrsException for an error that
    /// names none, with the error's id and category in its Data and its
    /// details inside it.
    /// </summary>
    public abstract class TransformBase : ArgumentTransformationAttribute
    {
        protected abstract NativeModule Module { get; }
        protected abstract uint TransformId { get; }

        public override object Transform(EngineIntrinsics engineIntrinsics, object inputData)
        {
            try
            {
                return Module.Transform(TransformId, inputData)!;
            }
            catch (Exception e) when (NamedException.TryRecordOf(e, out _, out _))
            {
                throw new ArgumentTransformationMetadataException(e.Message, e);
            }
            catch (PwrsException e)
            {
                // A panic: no error of the transform's, so no id to keep.
                throw new ArgumentTransformationMetadataException(e.Message);
            }
        }
    }

    /// <summary>
    /// Declared on every array-typed parameter: passes the binder the
    /// array a PSObject wraps, so an array of the parameter's own type
    /// binds as itself instead of being converted element by element.
    /// Any other value is returned unchanged.
    /// </summary>
    [AttributeUsage(AttributeTargets.Property | AttributeTargets.Field)]
    public sealed class UnwrapArrayAttribute : ArgumentTransformationAttribute
    {
        public override object Transform(EngineIntrinsics engineIntrinsics, object inputData) =>
            inputData is PSObject wrapped && wrapped.BaseObject is Array array ? array : inputData;
    }
}
