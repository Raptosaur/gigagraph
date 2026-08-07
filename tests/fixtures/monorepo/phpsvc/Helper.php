<?php

namespace Acme\Web;

function formatSlug(string $title): string
{
    return strtolower(str_replace(' ', '-', $title));
}
