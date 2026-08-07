<?php

namespace Acme\Web;

interface SlugStore
{
    public function persist(string $slug): bool;
}

class DbSlugStore implements SlugStore
{
    public function persist(string $slug): bool
    {
        return true;
    }
}

class Publisher
{
    private SlugStore $store;

    public function __construct(SlugStore $store)
    {
        $this->store = $store;
    }

    public function release(string $slug): bool
    {
        return $this->store->persist($slug);
    }
}

class CachingSlugStore implements SlugStore
{
    public function persist(string $slug): bool
    {
        return false;
    }
}

class StoreProvider
{
    public function register(): void
    {
        $this->app->bind(SlugStore::class, DbSlugStore::class);
    }
}
