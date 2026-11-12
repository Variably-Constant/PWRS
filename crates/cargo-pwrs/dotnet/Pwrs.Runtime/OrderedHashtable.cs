#if !NET
using System;
using System.Collections;
using System.Collections.Specialized;

namespace Pwrs
{
    /// <summary>
    /// A Hashtable that keeps its keys in the order they were added, for
    /// Windows PowerShell 5.1, which has no
    /// System.Management.Automation.OrderedHashtable. It has that class's
    /// shape: every member reads and writes one OrderedDictionary, so a key
    /// compares as a Hashtable's does, by case, a key removed and added
    /// again comes last, and Keys, Values, the enumerator, CopyTo and Clone
    /// follow the order. The Hashtable it derives from holds nothing; it is
    /// there so a script's `-is [hashtable]` and a [hashtable] parameter
    /// take the table as one.
    /// </summary>
    public sealed class OrderedHashtable : Hashtable, IEnumerable
    {
        private readonly OrderedDictionary _entries;

        public OrderedHashtable()
        {
            _entries = new OrderedDictionary();
        }

        public OrderedHashtable(int capacity)
        {
            _entries = new OrderedDictionary(capacity);
        }

        /// <summary>A table of <paramref name="dictionary"/>'s entries, in its order.</summary>
        public OrderedHashtable(IDictionary dictionary)
        {
            if (dictionary == null) throw new ArgumentNullException(nameof(dictionary));
            _entries = new OrderedDictionary(dictionary.Count);
            foreach (DictionaryEntry entry in dictionary)
            {
                _entries.Add(entry.Key, entry.Value);
            }
        }

        public override int Count => _entries.Count;

        public override bool IsFixedSize => false;

        public override bool IsReadOnly => false;

        public override bool IsSynchronized => false;

        public override ICollection Keys => _entries.Keys;

        public override ICollection Values => _entries.Values;

        /// <summary>The value under <paramref name="key"/>, or null when there is none.</summary>
        public override object? this[object key]
        {
            get => _entries[key];
            set => _entries[key] = value;
        }

        public override void Add(object key, object? value) => _entries.Add(key, value);

        public override void Clear() => _entries.Clear();

        /// <summary>Another table of this one's entries, in the same order.</summary>
        public override object Clone() => new OrderedHashtable(_entries);

        public override bool Contains(object key) => _entries.Contains(key);

        public override bool ContainsKey(object key) => _entries.Contains(key);

        public override bool ContainsValue(object? value)
        {
            foreach (DictionaryEntry entry in _entries)
            {
                if (Equals(entry.Value, value)) return true;
            }
            return false;
        }

        public override void CopyTo(Array array, int arrayIndex) => _entries.CopyTo(array, arrayIndex);

        public override IDictionaryEnumerator GetEnumerator() => _entries.GetEnumerator();

        IEnumerator IEnumerable.GetEnumerator() => _entries.GetEnumerator();

        public override void Remove(object key) => _entries.Remove(key);
    }
}
#endif
