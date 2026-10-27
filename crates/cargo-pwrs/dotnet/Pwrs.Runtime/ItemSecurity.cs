using System.Security.AccessControl;

namespace Pwrs
{
    /// <summary>
    /// The security descriptor of a provider item that is not a file, a
    /// directory or a registry key. Its access and audit rules give their
    /// rights as a number, since what each bit means is the provider's
    /// own. It is built as a container's descriptor, so the inheritance
    /// flags the provider's SDDL text gives an entry stay as given.
    /// </summary>
    public sealed class ItemSecurity : ObjectSecurity<int>
    {
        public ItemSecurity() : base(true, ResourceType.Unknown)
        {
        }
    }
}
