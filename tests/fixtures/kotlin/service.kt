package com.example.service

import com.example.model.User
import com.example.util.Formatter as Fmt
import kotlinx.coroutines.*

fun createUser(name: String, email: String): User {
    val normalized = normalize(email)
    return User(name, normalized)
}

fun normalize(email: String): String = email.trim().lowercase()

class UserService(private val repo: UserRepo) {
    fun register(name: String, email: String): User? {
        val user = createUser(name, email)
        val valid = this.validate(user)
        if (!valid) {
            return null
        }
        repo.save(user)
        return user
    }

    private fun validate(user: User): Boolean {
        fun hasDomain(email: String): Boolean {
            return email.contains("@")
        }
        return hasDomain(user.email)
    }

    fun renderAll(users: List<User>): List<String> {
        return users.map { render(it) }
    }

    private fun render(user: User): String = Fmt.pretty(user.name)

    suspend fun refresh(): Int {
        var count = 0
        while (count < 3) {
            delay(100)
            count += 1
        }
        return count
    }

    companion object {
        fun default(): UserService = UserService(UserRepo())
    }
}

object Registry {
    fun lookup(key: String): String = key.uppercase()
}

data class UserRepo(val entries: MutableList<User> = mutableListOf()) {
    fun save(user: User) {
        entries.add(user)
    }
}

fun String.initials(): String = this.split(" ").map { it.first() }.joinToString("")

infix fun Int.clampTo(max: Int): Int = if (this > max) max else this

operator fun UserRepo.plus(user: User): UserRepo {
    save(user)
    return this
}

fun retryTimes(times: Int, block: () -> Unit) {
    for (attempt in 0 until times) {
        try {
            block()
            return
        } catch (e: Exception) {
            println(e)
        }
    }
}

val defaultService = UserService.default()
