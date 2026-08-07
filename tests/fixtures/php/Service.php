<?php

namespace App\Services;

use App\Repositories\UserRepository;
use App\Support\Logger as Log;
use App\Models\{User, Account};

interface Notifier
{
    public function notify(int $userId, string $message): bool;
}

class MailerService implements Notifier
{
    private UserRepository $repo;

    public function __construct(UserRepository $repo)
    {
        $this->repo = $repo;
    }

    public function notify(int $userId, string $message): bool
    {
        $user = $this->repo->find($userId);
        if ($user === null) {
            Log::warn($message);
            return false;
        }
        $body = $this->render($user, $message);
        self::deliver($body);
        return true;
    }

    private function render(User $user, string $message): string
    {
        $label = match ($user->tier) {
            'gold' => strtoupper($user->name),
            default => $user->name,
        };
        return sprintf('%s: %s', $label, $message);
    }

    public static function deliver(string $body): void
    {
        $transport = new Transport();
        $transport->send($body);
        $account = new \App\Models\Account(1);
        $account?->touch();
    }
}
