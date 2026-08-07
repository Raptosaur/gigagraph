-- Core schema for the blog service.
CREATE TABLE users (
  id SERIAL PRIMARY KEY,
  email VARCHAR(255) NOT NULL,
  display_name TEXT,
  created_at TIMESTAMP DEFAULT now()
);

CREATE TABLE posts (
  id SERIAL PRIMARY KEY,
  author_id INT REFERENCES users(id),
  title TEXT NOT NULL,
  body TEXT,
  published BOOLEAN DEFAULT false
);

CREATE INDEX idx_posts_author ON posts (author_id);

CREATE VIEW author_activity AS
SELECT u.email,
       count(p.id) AS post_count,
       CASE WHEN count(p.id) > 10 THEN 'active' ELSE 'quiet' END AS status
FROM users u
LEFT JOIN posts p ON p.author_id = u.id
GROUP BY u.email;

CREATE FUNCTION posts_for(author_email VARCHAR) RETURNS SETOF posts AS $$
  SELECT p.* FROM posts p
  JOIN users u ON u.id = p.author_id
  WHERE u.email = author_email
$$ LANGUAGE sql;

INSERT INTO users (email) VALUES ('admin@example.com');
