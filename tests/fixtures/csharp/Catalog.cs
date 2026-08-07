using System;
using System.Collections.Generic;
using System.Text;
using System.Threading.Tasks;
using F = System.IO.File;
using static System.Math;

namespace Acme.Store
{
    public interface IDescribable
    {
        string Describe();
    }

    public class Catalog : IDescribable
    {
        private readonly List<Book> books = new List<Book>();
        private int capacity;

        public Catalog(int capacity)
        {
            this.capacity = capacity;
            Console.WriteLine("catalog created");
        }

        public Catalog() : this(16)
        {
        }

        public int Count() => books.Count;

        public static Catalog WithDefaults()
        {
            var catalog = new Catalog(8);
            catalog.AddBook(new Book("Dune", 1965));
            return catalog;
        }

        public void AddBook(Book book)
        {
            if (books.Count >= capacity)
            {
                Grow();
            }
            books.Add(book);
        }

        public void AddBook(string title, int year)
        {
            this.AddBook(new Book(title, year));
        }

        private void Grow()
        {
            capacity = Max(capacity * 2, 16);
        }

        public string Describe()
        {
            var sb = new StringBuilder();
            foreach (var book in books)
            {
                sb.Append(book.Label());
            }
            var text = sb.ToString().Trim();
            Console.WriteLine(text);
            return text;
        }

        public async Task<int> LoadAsync(string path)
        {
            var text = await F.ReadAllTextAsync(path);
            int CountLines(string s) => s.Split('\n').Length;
            return CountLines(text);
        }

        public List<string> ModernTitles()
        {
            var titles = new List<string>();
            for (int i = 0; i < books.Count; i++)
            {
                var book = books[i];
                titles.Add(book.IsModern() ? book.Label() : "classic");
            }
            Console.WriteLine(this.First<string>(titles));
            return titles;
        }

        public T First<T>(List<T> items)
        {
            if (items.Count == 0)
            {
                throw new InvalidOperationException("empty");
            }
            return items[0];
        }
    }

    public record Book(string Title, int Year)
    {
        public string Label() => $"{Title} ({Year})";

        public bool IsModern()
        {
            return Year >= 1950;
        }
    }
}
