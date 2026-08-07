<?php

// PHP 7.0-7.4 era syntax coverage (see tests/php_test.rs).
// Findings recorded from AST probes of tree-sitter-php against this file:
// - `?Foo` parameter/return types parse as (optional_type (named_type ...));
//   see the optional_type query patterns in src/lang/php.rs.
// - Untyped properties with only a docblock `@var` are property_declaration
//   nodes WITHOUT a `type:` field; the `$this->prop = $param` ctor join is
//   what recovers their type.
// - `new class(...) {...}` is an object_creation_expression whose
//   declaration_list is NOT a TYPE_KINDS ancestor, so methods inside an
//   anonymous class inherit the ENCLOSING named class/nothing — verified
//   harmless for the methods that FOLLOW the anonymous class.

namespace App\Legacy;

use App\Repositories\OrderRepository;

interface Auditor
{
    public function audit(string $event): void;
}

class LegacyReportService
{
    /** @var OrderRepository */
    private $orders;

    private ?Auditor $auditor;

    public function __construct(OrderRepository $orders, ?Auditor $auditor)
    {
        $this->orders = $orders;
        $this->auditor = $auditor;
    }

    public function summarize(?string $region, int $limit = 10): ?array
    {
        $region = $region ?? 'all';
        $rows = $this->orders->findByRegion($region, $limit);
        [$first, $second] = $rows;
        list($head, $tail) = $rows;
        usort($rows, function ($a, $b) {
            return $a <=> $b;
        });
        $this->auditor->audit('summarize');
        return $rows;
    }

    public function renderBanner(string $title): string
    {
        $body = <<<HTML
<div class="banner">
  <h1>$title</h1>
</div>
HTML;
        return trim($body);
    }

    public function makeInlineAuditor(): Auditor
    {
        return new class implements Auditor {
            public function audit(string $event): void
            {
                error_log($event);
            }
        };
    }

    public function afterAnonymous(int $x): int
    {
        return intdiv($x, 2);
    }
}

class TypedCounters
{
    public int $hits = 0;
    public ?string $label;

    public function bump(int $by): int
    {
        $add = fn(int $n): int => $n + $by;
        $this->hits = $add($this->hits);
        return $this->hits;
    }
}

function legacy_percent(?float $part, float $whole): float
{
    $part = $part ?? 0.0;
    return round($part / $whole * 100, 2);
}
