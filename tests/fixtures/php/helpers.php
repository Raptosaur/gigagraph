<?php

require_once 'bootstrap.php';
include 'legacy/compat.php';

function slugify(string $title): string
{
    $clean = strtolower(trim($title));
    return str_replace(' ', '-', $clean);
}

function chunk_lines(array $lines, int $size): array
{
    $out = [];
    $buf = [];
    foreach ($lines as $line) {
        $buf[] = $line;
        if (count($buf) === $size) {
            $out[] = $buf;
            $buf = [];
        }
    }
    if ($buf !== []) {
        $out[] = $buf;
    }
    return $out;
}

$shout = fn(string $word) => strtoupper($word);

$emit = function (string $prefix, string $line) {
    echo trim($prefix) . $line;
};

$slug = slugify('Hello World');
print_r(chunk_lines([$slug], 2));
