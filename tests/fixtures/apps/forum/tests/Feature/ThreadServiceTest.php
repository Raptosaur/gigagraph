<?php

namespace Tests\Feature;

use App\Services\ThreadService;
use PHPUnit\Framework\Attributes\DataProvider;
use PHPUnit\Framework\Attributes\Test;
use PHPUnit\Framework\TestCase;

class ThreadServiceTest extends TestCase
{
    private ThreadService $service;

    protected function setUp(): void
    {
        $this->service = new ThreadService();
    }

    public function testOpenReturnsASlug(): void
    {
        $thread = $this->service->open('Hello World', 'body');
        $this->assertSame('hello-world', $thread['slug']);
    }

    public function testCloseReturnsFalseForUnknownSlug(): void
    {
        $this->assertFalse($this->service->close('nope'));
    }

    #[Test]
    public function itListsRecentThreads(): void
    {
        $this->service->open('One', 'a');
        $this->assertCount(1, $this->service->recent(10));
    }

    #[DataProvider('titles')]
    public function testSlugifyNormalises(string $title, string $expected): void
    {
        $this->assertSame($expected, ThreadService::slugify($title));
    }

    public static function titles(): array
    {
        return [['A B', 'a-b'], ['  Spaced  ', 'spaced']];
    }
}
