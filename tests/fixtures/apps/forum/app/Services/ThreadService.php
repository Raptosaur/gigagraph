<?php

namespace App\Services;

class ThreadService
{
    /** @var array<string, array{title: string, body: string, replies: int}> */
    private array $threads = [];

    public function open(string $title, string $body): array
    {
        $slug = self::slugify($title);
        $this->threads[$slug] = ['title' => $title, 'body' => $body, 'replies' => 0];
        return $this->threads[$slug] + ['slug' => $slug];
    }

    public function bySlug(string $slug): ?array
    {
        return $this->threads[$slug] ?? null;
    }

    public function recent(int $limit): array
    {
        return array_slice($this->threads, 0, $limit, true);
    }

    public function close(string $slug): bool
    {
        if (!isset($this->threads[$slug])) {
            return false;
        }
        unset($this->threads[$slug]);
        return true;
    }

    public static function slugify(string $title): string
    {
        return trim(preg_replace('/[^a-z0-9]+/', '-', strtolower($title)), '-');
    }
}

function reply_count(array $thread): int
{
    return $thread['replies'] ?? 0;
}
