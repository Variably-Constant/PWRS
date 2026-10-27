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
    /// ArgumentTransformationMetadataException produces.
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
            catch (PwrsException e)
            {
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
