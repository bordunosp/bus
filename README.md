use twox_hash::XxHash3_64;
use std::hash::Hasher;

let mut hasher = XxHash3_64::with_seed(0);
hasher.write(payload_str.as_bytes());
let hash_u64 = hasher.finish();

// Для MySQL:
// params.push(hash_u64.into());

// Для Postgres (обхід відсутності unsigned):
// params.push((hash_u64 as i64).into());





наскільки ти добре знаєш код та документацію Oban з Elixir?

ми будемо на його основі робити bus для rust

ми плануємо використовувати

- inventory для автоматичного пошуку хендлерів для івента
- postcard для бінарногозберігання в бд
- serde_json для зберігання в бд json виключно для дебагу
- на 1 івент в нас будуть багато хендлерів
- для бд використовуємо sqlx
- ми так само будемо реалізовувати варіант крону
- у нас 2 різних варіанта для хендлерів
    - in memmory
    - out of box

при цьому все максимально близько до Oban але з урахуванням rust підходів
це має бути бібліотека яка працюватиме на будьякому проекті rust
код має бути максимально продуктивним та архітектурно правильним
мінімум алокацій + максимум перевірених підїодів + бест практіс + класичні патерни

почнемо з міграцій

напиши мені повну структуру всіх необхідних таблиць які нам треба
з усіма індексами і тд для постгреса та окремо мускуля

```rust
BusQueueConfigurationBuilder::new()
        .connection("postgres://localhost/db")
        .add_queue("critical", QueueConfigBuilder::new()
            .workers(10)
            .max_attempts(5)
            .execution_timeout(chrono::Duration::minutes(1)))
        .add_queue("default", QueueConfigBuilder::new().workers(2))
        .build()?;
```


Важливий нюанс (який мало хто знає)
У нових версіях Oban унікальність гарантується через PostgreSQL advisory locks + partial index, а не тільки через select.
Це робить операцію race-condition safe.

https://hexdocs.pm/oban/Oban.Plugins.Cron.html

    // Plugins
    // Oban.Plugin
    // Oban.Plugins.Cron
    // Oban.Plugins.Lifeline
    // Oban.Plugins.Pruner
    // Oban.Plugins.Reindexer



```rust
pub async fn event<'a, TContext, TEvent>(
  mut ctx: TContext,
  event: TEvent,
) where TContext: IBusContext<'a>
{
  // Намагаємось отримати мутабельний доступ
  if let Some(m_ctx) = ctx.as_mut() {
    // Отут ми міняємо поля, бо у нас є &mut
    // Це спрацює ТІЛЬКИ якщо юзер передав event(&mut ctx, ...)
    m_ctx.increment_some_counter();
  } else {
    // Юзер передав event(&ctx, ...), міняти не можна.
    // Можна або нічого не робити, або залогувати trace.
  }

  // Далі викликаємо логіку, яка працює для обох типів (read-only)
  ctx.metadata();
}





pub trait IBusContext<'a> {
  // ... твої методи ...

  // Додаємо цей метод:
  fn as_mut(&mut self) -> Option<&mut Self> {
    None // За замовчуванням — не можна
  }
}

// Для звичайного посилання — мутабельність завжди None
impl<'a, 't, T: IBusContext<'a> + ?Sized> IBusContext<'a> for &'t T {
  fn as_mut(&mut self) -> Option<&mut Self> {
    None
  }
  // ... решта методів ...
}

// Для мутабельного посилання — повертаємо Some(*self)
impl<'a, 't, T: IBusContext<'a> + ?Sized> IBusContext<'a> for &'t mut T {
  fn as_mut(&mut self) -> Option<&mut Self> {
    Some(self) // Повертаємо посилання на себе
  }
  // ... решта методів ...
}
```