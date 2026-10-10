using System.Drawing;
using System.Management.Automation;
using System.Runtime.CompilerServices;
using System.Windows.Forms;

namespace Pwrs.Modules.Hello
{
    /// <summary>
    /// A hand-written cmdlet using desktop assemblies the module names under
    /// `references`, without showing a window: Windows Forms'
    /// SystemInformation, for the computer name it reports and the version
    /// of the System.Windows.Forms assembly the host bound, and a
    /// System.Drawing bitmap, drawn into and read back, with the name of the
    /// assembly Bitmap came from. Off Windows no host carries Windows Forms,
    /// and the call fails naming it.
    /// </summary>
    [Cmdlet(VerbsCommon.Get, "RustHybridDesktop")]
    public sealed class GetRustHybridDesktopCommand : PSCmdlet
    {
        protected override void ProcessRecord()
        {
            var info = new PSObject();
            info.Properties.Add(new PSNoteProperty("ComputerName", SystemInformation.ComputerName));
            info.Properties.Add(new PSNoteProperty("FormsVersion", typeof(SystemInformation).Assembly.GetName().Version?.ToString()));
            AddDrawing(info);
            WriteObject(info);
        }

        /// <summary>
        /// Sets one pixel of a 4 by 3 bitmap and reads it back. A method of
        /// its own, so System.Drawing is loaded only once Windows Forms has
        /// been, and a host without either reports Windows Forms.
        /// </summary>
        [MethodImpl(MethodImplOptions.NoInlining)]
        private static void AddDrawing(PSObject info)
        {
            using (var bitmap = new Bitmap(4, 3))
            {
                bitmap.SetPixel(1, 1, Color.FromArgb(255, 10, 20, 30));
                info.Properties.Add(new PSNoteProperty("BitmapSize", bitmap.Width + "x" + bitmap.Height));
                info.Properties.Add(new PSNoteProperty("PixelGreen", (int)bitmap.GetPixel(1, 1).G));
                info.Properties.Add(new PSNoteProperty("DrawingAssembly", typeof(Bitmap).Assembly.GetName().Name));
            }
        }
    }
}
