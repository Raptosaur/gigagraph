CREATE TABLE authors (
    id   SERIAL PRIMARY KEY,
    name TEXT NOT NULL
);

CREATE TABLE books (
    id          SERIAL PRIMARY KEY,
    isbn        TEXT NOT NULL UNIQUE,
    title       TEXT NOT NULL,
    price_cents INTEGER NOT NULL,
    author_id   INTEGER NOT NULL REFERENCES authors (id)
);

CREATE VIEW book_catalog AS
SELECT b.id, b.title, a.name AS author
FROM books b
JOIN authors a ON a.id = b.author_id;

CREATE FUNCTION cart_total(cart INTEGER) RETURNS INTEGER AS $$
    SELECT COALESCE(SUM(b.price_cents * l.quantity), 0)
    FROM cart_lines l
    JOIN books b ON b.id = l.book_id
    WHERE l.cart_id = cart;
$$ LANGUAGE SQL;

CREATE PROCEDURE prune_empty_carts() LANGUAGE SQL AS $$
    DELETE FROM carts c WHERE NOT EXISTS (
        SELECT 1 FROM cart_lines l WHERE l.cart_id = c.id
    );
$$;
