<?php

namespace Acme\Web;

use Monolog\Logger;

class Service
{
    public function publish(string $title): string
    {
        $slug = formatSlug($title);
        $log = new Logger('web');
        $log->info($slug);
        return $slug;
    }
}
