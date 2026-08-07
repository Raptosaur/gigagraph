<?php

// Symfony 5 annotation-era controller (PHP 7 docblock @Route, no PHP 8
// attributes). Shapes distilled from symfony/demo v1.7.0 (Symfony 5.3):
// - CLASS-level docblock @Route("/member") prefix joined onto every method
//   route via the php.rs comment ride-along (see src/lang/php.rs) —
//   demo's BlogController/UserController/Admin\BlogController all do this.
// - methods="GET|POST" pipe-string form (demo uses it throughout) alongside
//   the methods={"GET"} brace-list form.
// - requirements={}/defaults={} noise and inline `{id<\d+>}` requirements
//   must not break path extraction.

namespace App\Controller;

use Symfony\Bundle\FrameworkBundle\Controller\AbstractController;
use Symfony\Component\Routing\Annotation\Route;

/**
 * Controller used to manage member profiles.
 *
 * @Route("/member")
 */
class ProfileController extends AbstractController
{
    /**
     * @Route("/preferences", methods="GET|POST", name="member_preferences")
     */
    public function preferences(?string $tab): string
    {
        return '';
    }

    /**
     * @Route("/badges/{id<\d+>}", methods={"GET"}, requirements={"id"="\d+"}, defaults={"page": "1"})
     */
    public function badge($id): string
    {
        return '';
    }
}
