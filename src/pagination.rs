use serde::Deserialize;

pub const DEFAULT_PER_PAGE: i64 = 25;

#[derive(Debug, Clone)]
pub struct Paginated<T> {
    pub items: Vec<T>,
    pub page: i64,
    pub per_page: i64,
    pub total: i64,
}

impl<T> Paginated<T> {
    pub fn total_pages(&self) -> i64 {
        if self.total == 0 {
            1
        } else {
            (self.total + self.per_page - 1) / self.per_page
        }
    }

    pub fn has_prev(&self) -> bool {
        self.page > 1
    }

    pub fn has_next(&self) -> bool {
        self.page < self.total_pages()
    }

    pub fn offset(&self) -> i64 {
        (self.page - 1) * self.per_page
    }

    pub fn showing_start(&self) -> i64 {
        if self.total == 0 {
            0
        } else {
            self.offset() + 1
        }
    }

    pub fn showing_end(&self) -> i64 {
        std::cmp::min(self.offset() + self.per_page, self.total)
    }

    pub fn page_numbers(&self) -> Vec<PageItem> {
        let total = self.total_pages();
        let current = self.page;
        let mut items = Vec::new();

        if total <= 7 {
            for i in 1..=total {
                items.push(PageItem::Page(i));
            }
        } else {
            items.push(PageItem::Page(1));
            if current > 3 {
                items.push(PageItem::Dots);
            }
            let start = std::cmp::max(2, current - 1);
            let end = std::cmp::min(total - 1, current + 1);
            for i in start..=end {
                items.push(PageItem::Page(i));
            }
            if current < total - 2 {
                items.push(PageItem::Dots);
            }
            items.push(PageItem::Page(total));
        }
        items
    }
}

#[derive(Debug, Clone)]
pub enum PageItem {
    Page(i64),
    Dots,
}

#[derive(Debug, Deserialize)]
pub struct PageParams {
    #[serde(default = "default_page")]
    pub page: i64,
}

fn default_page() -> i64 {
    1
}

impl PageParams {
    pub fn validated(&self) -> i64 {
        if self.page < 1 { 1 } else { self.page }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn total_pages_math() {
        let p: Paginated<()> = Paginated { items: vec![], page: 1, per_page: 25, total: 0 };
        assert_eq!(p.total_pages(), 1);
        let p: Paginated<()> = Paginated { items: vec![], page: 1, per_page: 25, total: 25 };
        assert_eq!(p.total_pages(), 1);
        let p: Paginated<()> = Paginated { items: vec![], page: 1, per_page: 25, total: 26 };
        assert_eq!(p.total_pages(), 2);
        let p: Paginated<()> = Paginated { items: vec![], page: 1, per_page: 25, total: 100 };
        assert_eq!(p.total_pages(), 4);
    }

    #[test]
    fn showing_range() {
        let p: Paginated<()> = Paginated { items: vec![], page: 2, per_page: 25, total: 60 };
        assert_eq!(p.showing_start(), 26);
        assert_eq!(p.showing_end(), 50);
    }

    #[test]
    fn page_numbers_small() {
        let p: Paginated<()> = Paginated { items: vec![], page: 1, per_page: 25, total: 100 };
        assert_eq!(p.page_numbers().len(), 4);
    }

    #[test]
    fn page_numbers_large() {
        let p: Paginated<()> = Paginated { items: vec![], page: 5, per_page: 25, total: 500 };
        let nums = p.page_numbers();
        assert!(nums.len() <= 9);
    }
}
