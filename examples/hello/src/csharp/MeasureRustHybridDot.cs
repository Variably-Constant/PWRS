using System;
using System.Management.Automation;
using System.Numerics;
using System.Runtime.InteropServices;
#if NET8_0_OR_GREATER
using System.Runtime.Intrinsics;
#endif

namespace Pwrs.Modules.Hello
{
    /// <summary>
    /// A hand-written cmdlet taking the dot product of two integer arrays
    /// the way C# written for both hosts does: through
    /// System.Runtime.Intrinsics where the build defines NET8_0_OR_GREATER
    /// and the hardware accelerates Vector256, otherwise through
    /// System.Numerics.Vector&lt;T&gt; over spans, the one hardware vector
    /// type .NET Framework can reach, and a scalar loop for whatever the
    /// lanes leave over. It answers the sum, the path taken, that path's
    /// lane count, and whether the intrinsics path was compiled in.
    /// </summary>
    [Cmdlet(VerbsDiagnostic.Measure, "RustHybridDot")]
    public sealed class MeasureRustHybridDotCommand : PSCmdlet
    {
        [Parameter(Mandatory = true, Position = 0)]
        public int[] Left { get; set; } = new int[0];

        [Parameter(Mandatory = true, Position = 1)]
        public int[] Right { get; set; } = new int[0];

        protected override void ProcessRecord()
        {
            if (Left.Length != Right.Length)
            {
                var reason = new ArgumentException("Left and Right must be the same length.");
                ThrowTerminatingError(new ErrorRecord(reason, "PwrsHybridDotLength", ErrorCategory.InvalidArgument, null));
            }
            ReadOnlySpan<int> left = Left;
            ReadOnlySpan<int> right = Right;
            int sum = 0;
            int done = 0;
            int lanes = 0;
            string path = "Scalar";
            bool intrinsics = false;
#if NET8_0_OR_GREATER
            intrinsics = true;
            if (Vector256.IsHardwareAccelerated)
            {
                lanes = Vector256<int>.Count;
                path = "Vector256";
                Vector256<int> acc = Vector256<int>.Zero;
                for (; done + lanes <= left.Length; done += lanes)
                {
                    acc += Vector256.Create(left.Slice(done, lanes)) * Vector256.Create(right.Slice(done, lanes));
                }
                sum = Vector256.Sum(acc);
            }
            else
#endif
            if (Vector.IsHardwareAccelerated)
            {
                lanes = Vector<int>.Count;
                path = "Vector";
                ReadOnlySpan<Vector<int>> leftLanes = MemoryMarshal.Cast<int, Vector<int>>(left);
                ReadOnlySpan<Vector<int>> rightLanes = MemoryMarshal.Cast<int, Vector<int>>(right);
                Vector<int> acc = Vector<int>.Zero;
                for (int k = 0; k < leftLanes.Length; k++)
                {
                    acc += leftLanes[k] * rightLanes[k];
                }
                sum = Vector.Dot(acc, Vector<int>.One);
                done = leftLanes.Length * lanes;
            }
            for (; done < left.Length; done++)
            {
                sum += left[done] * right[done];
            }
            var result = new PSObject();
            result.Properties.Add(new PSNoteProperty("Sum", sum));
            result.Properties.Add(new PSNoteProperty("Path", path));
            result.Properties.Add(new PSNoteProperty("Lanes", lanes));
            result.Properties.Add(new PSNoteProperty("Intrinsics", intrinsics));
            WriteObject(result);
        }
    }
}
