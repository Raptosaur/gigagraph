<?php

namespace App\Controller;

use Symfony\Component\Routing\Attribute\Route;

class UserController
{
    #[Route('/profiles/{id}', methods: ['GET', 'PUT'])]
    public function show(int $id): string
    {
        return '';
    }
}
