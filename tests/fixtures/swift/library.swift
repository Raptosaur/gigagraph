import Foundation
import UIKit.UIView

struct Book {
    let title: String
    let pages: Int

    init(title: String, pages: Int) {
        self.title = title
        self.pages = pages
    }

    func summary() -> String {
        return title.uppercased()
    }

    static func placeholder() -> Book {
        return Book(title: "Untitled", pages: 0)
    }
}

class Library {
    var books: [Book] = []

    func add(_ book: Book) {
        books.append(book)
    }

    func titles() -> [String] {
        return books.map { $0.title }
    }

    func findBook(named name: String) -> Book? {
        for book in books {
            if book.title == name {
                return book
            }
        }
        return nil
    }

    class func makeDefault() -> Library {
        let library = Library()
        library.add(Book.placeholder())
        return library
    }
}

enum Genre {
    case fiction
    case reference

    func label() -> String {
        switch self {
        case .fiction:
            return "Fiction"
        case .reference:
            return "Reference"
        }
    }
}

extension Library {
    func loadSamples(from names: [String]) {
        names.forEach { name in
            self.add(Book(title: name, pages: 100))
        }
    }
}

func slugify(_ raw: String) -> String {
    guard !raw.isEmpty else { return "untitled" }
    return raw.lowercased().replacingOccurrences(of: " ", with: "-")
}

func catalog(_ library: Library) -> String {
    var lines: [String] = []
    while lines.isEmpty {
        lines = library.titles().map { slugify($0) }
    }
    return lines.joined(separator: "\n")
}

let shared = Library.makeDefault()
print(catalog(shared))
