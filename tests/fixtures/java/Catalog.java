package com.example.library;

import java.util.ArrayList;
import java.util.List;
import java.util.function.*;
import static java.lang.Math.max;

public class Catalog {
    private final List<Book> books = new ArrayList<>();
    private int capacity;

    public Catalog() {
        this(16);
    }

    public Catalog(int capacity) {
        this.capacity = capacity;
    }

    public void addBook(Book book) {
        if (books.size() >= capacity) {
            grow();
        }
        books.add(book);
    }

    public void addBook(String title, int year) {
        this.addBook(new Book(title, year));
    }

    private void grow() {
        capacity = max(capacity * 2, 32);
    }

    public static Catalog withDefaults() {
        Catalog catalog = new Catalog();
        catalog.addBook("Dune", 1965);
        return catalog;
    }

    public <T extends Comparable<T>> T pickLarger(T a, T b) {
        return a.compareTo(b) >= 0 ? a : b;
    }

    public String summarize() {
        StringBuilder sb = new StringBuilder();
        for (Book book : books) {
            sb.append(book.describe()).append('\n');
        }
        String result = sb.toString().trim();
        System.out.println(result);
        return result;
    }

    public List<String> titles() {
        List<String> out = new ArrayList<>();
        books.forEach(book -> out.add(book.title()));
        return out;
    }

    public void forEachTitle(Consumer<String> action) {
        for (String title : titles()) {
            action.accept(title);
        }
    }

    public Runnable auditTask() {
        return new Runnable() {
            @Override
            public void run() {
                summarize();
            }
        };
    }

    public class Shelf {
        public int countFiction() {
            int count = 0;
            for (Book book : books) {
                if (book.isModern()) {
                    count++;
                }
            }
            return count;
        }
    }

    public static class Stats {
        public static double averageYear(List<Book> sample) {
            double total = 0;
            for (Book book : sample) {
                total += book.year();
            }
            return sample.isEmpty() ? 0 : total / sample.size();
        }
    }
}

interface Describable {
    String describe();

    default String shortLabel() {
        return describe().substring(0, 8);
    }
}

record Book(String title, int year) implements Describable {
    public String describe() {
        return title + " (" + year + ")";
    }

    public boolean isModern() {
        return year >= 2000;
    }
}
