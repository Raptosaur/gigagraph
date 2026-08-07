// Spring-style DI wiring: interface, annotated implementation, and a
// consumer with a ctor-injected typed field used through `this.<field>`.
// Exercised by tests/java_test.rs (extracts_java_di_type_captures).
package com.example.wiring;

import java.util.List;

interface UserStore {
    void save(String user);
}

interface Lifecycle extends AutoCloseable {
}

@Component
class DbUserStore implements UserStore {
    public void save(String user) {
        System.out.println(user);
    }
}

class SignupService extends BaseService implements Lifecycle, Auditable {
    private final UserStore store;
    private List<Signup> pending;
    @Autowired
    private Mailer mailer;
    private int retries;

    SignupService(UserStore store, Mailer mailer) {
        this.store = store;
        this.mailer = mailer;
    }

    void register(String user) {
        Greeter greeter = new Greeter();
        var clock = new Clock();
        this.store.save(user);
        greeter.hello(user);
    }
}

record AuditEvent(String kind) implements Auditable {
    public void audit() {
        System.out.println(kind);
    }
}
